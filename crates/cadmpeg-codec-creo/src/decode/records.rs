// SPDX-License-Identifier: Apache-2.0
//! Typed native record projections from `ContainerScan`.

use std::collections::{BTreeSet, HashMap, HashSet};
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
    binary_flag_value, feature_definition_has_sketch_design, sketch_table_headers,
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
    body: &'a [u8],
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

pub(super) fn reference_line_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<CreoReferenceLineRecord>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let family = |kind: &crate::reference::ReferenceLineKind| match kind {
            crate::reference::ReferenceLineKind::Line => "line",
            crate::reference::ReferenceLineKind::Line3d { .. } => "line3d",
        };
        let mut records = Vec::new();
        for line in ctx.admit_iter(&scan.references.lines, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:mdl_ref_info:{}_record#{}",
                    family(line.kind()),
                    line.offset
                ),
                "creo native reference line IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native reference line records")?;
            records.push(CreoReferenceLineRecord {
                id,
                kind: line.kind().clone(),
                start: line.start().get().into(),
                end: line.end().get().into(),
                offset: line.offset,
            });
        }
        Ok(records)
    })
}

pub(super) fn reference_circle_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<CreoReferenceCircleRecord>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for circle in ctx.admit_iter(&scan.references.circles, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:mdl_ref_info:arc_z_record#{}", circle.offset),
                "creo native reference circle IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native reference circle records")?;
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
        Ok(records)
    })
}

pub(super) fn reference_conic_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<
    (
        Vec<CreoReferenceConicRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for conic in ctx.admit_iter(&scan.references.conics, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:mdl_ref_info:conic_record#{}", conic.offset),
                "creo native reference conic IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native reference conic records")?;
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
    })
}

pub(super) fn reference_ellipse_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<CreoReferenceEllipseRecord>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for ellipse in ctx.admit_iter(&scan.references.ellipses, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:mdl_ref_info:ellipse_carrier#{}", ellipse.offset),
                "creo native reference ellipse IDs",
            )?;
            let source_conic_id = ctx.format_retained(
                format_args!("creo:mdl_ref_info:conic_record#{}", ellipse.offset),
                "creo native reference ellipse source IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native reference ellipse records")?;
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
        Ok(records)
    })
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

pub(super) fn expanded_section_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<CreoExpandedSectionRecord>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for section in ctx.admit_iter(
            &scan.framing.expanded_sections,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:container:expanded_section#{}:{}",
                    section.name, section.source_offset
                ),
                "creo native expanded section IDs",
            )?;
            let name =
                ctx.copy_retained_text(&section.name, "creo native expanded section names")?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(section.data.len()),
                "creo expanded section digest",
            )?;
            let sha256 = ctx.format_retained(
                format_args!("{}", HexDigest(sha256(&section.data))),
                "creo native expanded section hashes",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native expanded section records")?;
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
    })
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

pub(super) fn feature_entity_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureEntityRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for entity in ctx.admit_iter(&scan.features.entities, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:allfeatur:entity#{}", entity.entity_id),
                "creo feature entity record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature entity records")?;
            records.push(CreoFeatureEntityRecord {
                id,
                entity_id: entity.entity_id,
                type_byte: entity.type_byte,
                name: &entity.name,
                offset: entity.offset,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_entity_reference_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureEntityReferenceRecord>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for reference in ctx.admit_iter(
            &scan.features.entity_references,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:allfeatur:entity_reference#{}", reference.offset),
                "creo feature entity reference record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature entity reference records")?;
            records.push(CreoFeatureEntityReferenceRecord {
                id,
                source_entity_id: reference.source_entity_id,
                target_entity_id: reference.target_entity_id,
                target_resolved: cadmpeg_core::decode::index_from_u32(reference.target_entity_id)
                    < scan.features.entities.len(),
                offset: reference.offset,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_entity_table_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureEntityTableRecord>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for table in ctx.admit_iter(&scan.features.entity_tables, "creo native record traversal")? {
            let mut entry_ids = Vec::new();
            let mut entries = Vec::new();
            let mut surface_ids = Vec::new();
            let mut non_surface_entity_ids = Vec::new();
            for entry in ctx.admit_iter(table.entries.as_slice(), "creo native record traversal")? {
                ctx.reserve_vec(
                    &mut entry_ids,
                    1,
                    "creo feature entity table record entry ids",
                )?;
                entry_ids.push(entry.entity_id);
                ctx.reserve_vec(&mut entries, 1, "creo feature entity table record entries")?;
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
                if ctx.contains_btree_set(
                    table.unique_surface_ids(),
                    &entry.entity_id,
                    "creo entity table surface membership",
                )? {
                    ctx.reserve_vec(
                        &mut surface_ids,
                        1,
                        "creo feature entity table record surface ids",
                    )?;
                    surface_ids.push(entry.entity_id);
                } else {
                    ctx.reserve_vec(
                        &mut non_surface_entity_ids,
                        1,
                        "creo feature entity table record non surface ids",
                    )?;
                    non_surface_entity_ids.push(entry.entity_id);
                }
            }
            let id = ctx.format_retained(
                format_args!("creo:allfeatur:entity_table#{}", table.offset),
                "creo feature entity table record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature entity table records")?;
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
    })
}

pub(super) fn feature_geometry_table_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureGeometryTableRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for table in ctx.admit_iter(
            &scan.features.geometry_tables,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:feature:geometry_table#{}", table.offset),
                "creo feature geometry table record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature geometry table records")?;
            records.push(CreoFeatureGeometryTableRecord {
                id,
                owner_feature_id: table.feature_id,
                kind: &table.kind,
                declared_count: table.count,
                entity_class_id: table.entity_class,
                offset: table.offset,
                source_section: source_section_ref(ctx, scan, table.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_loop_history_entry_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureLoopHistoryEntryRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for entry in ctx.admit_iter(
            &scan.features.loop_history_entries,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:feature:loop_history_entry#{}", entry.offset),
                "creo feature loop history record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature loop history records")?;
            records.push(CreoFeatureLoopHistoryEntryRecord {
                id,
                owner_feature_id: entry.feature_id,
                ordinal: entry.ordinal,
                loop_id: entry.loop_id,
                field_bytes: entry,
                boundary: &entry.boundary,
                offset: entry.offset,
                end_offset: entry.end_offset,
                source_section: source_section_ref(ctx, scan, entry.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_affected_id_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureAffectedIdsRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(&scan.features.affected_ids, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:feature:affected_ids#{}", record.offset),
                "creo feature affected ids record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature affected ids records")?;
            records.push(CreoFeatureAffectedIdsRecord {
                id,
                owner_feature_id: record.feature_id,
                kind: affected_kind(record.kind),
                ids: &record.ids,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_replay_affected_id_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureReplayAffectedIdsRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.features.replay_affected_ids,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:feature:replay_affected_ids#{}", record.offset),
                "creo feature replay affected ids record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature replay affected ids records")?;
            records.push(CreoFeatureReplayAffectedIdsRecord {
                id,
                owner_feature_id: record.feature_id,
                geometry_ids: &record.geometry_ids,
                edge_ids: &record.edge_ids,
                geometry_extent: extent_source(record.geometry_extent),
                edge_extent: extent_source(record.edge_extent),
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn surface_merge_replay_affected_id_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoSurfaceMergeReplayAffectedIdsRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.features.surface_merge_replay_affected_ids,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:feature:surface_merge_replay_affected_ids#{}",
                    record.offset
                ),
                "creo surface merge replay affected ids record id",
            )?;
            ctx.reserve_vec(
                &mut records,
                1,
                "creo surface merge replay affected ids records",
            )?;
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
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_loop_restore_direction_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureLoopRestoreDirectionRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.features.loop_restore_directions,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:feature:loop_restore_direction#{}", record.offset),
                "creo feature loop restore direction record id",
            )?;
            ctx.reserve_vec(
                &mut records,
                1,
                "creo feature loop restore direction records",
            )?;
            records.push(CreoFeatureLoopRestoreDirectionRecord {
                id,
                owner_feature_id: record.feature_id,
                lane: match record.lane {
                    crate::feature::rows::LoopRestoreDirectionLane::Primary => "primary",
                    crate::feature::rows::LoopRestoreDirectionLane::Secondary => "secondary",
                },
                value: record.value,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_revolution_extent_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureRevolutionExtentRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.features.revolution_extents,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:feature:revolution_extent#{}", record.offset),
                "creo feature revolution extent record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature revolution extent records")?;
            records.push(CreoFeatureRevolutionExtentRecord {
                id,
                owner_feature_id: record.feature_id,
                kind: "full_turn",
                angle_radians: std::f64::consts::TAU,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_choice_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureChoiceRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for choice in ctx.admit_iter(&scan.features.choices, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:feature:choice#{}", choice.offset),
                "creo feature choice record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature choice records")?;
            records.push(CreoFeatureChoiceRecord {
                id,
                owner_feature_id: choice.feature_id,
                label: &choice.label,
                type_byte: choice.type_byte,
                payload: &choice.payload,
                payload_offset: choice.payload_offset,
                offset: choice.offset,
                source_section: source_section_ref(ctx, scan, choice.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_row_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureRowRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for row in ctx.admit_iter(&scan.features.rows, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:allfeatur:feature_row#{}", row.offset),
                "creo feature row record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo feature row records")?;
            records.push(CreoFeatureRowRecord {
                id,
                owner_feature_id: row.feature_id,
                header: row.body.header(),
                root_schema_class: row.root_schema_class.map(SchemaClass::code),
                stream_offset: row.stream_offset,
                body: &row.body,
                body_offset: row.body_offset,
                offset: row.offset,
                source_section: source_section_ref(ctx, scan, row.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn depdb_recipe_row_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureRowRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for row in ctx.admit_iter(
            &scan.features.depdb_recipe_rows,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:depdb:recipe_row#{}", row.offset),
                "creo depdb recipe row record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo depdb recipe row records")?;
            records.push(CreoFeatureRowRecord {
                id,
                owner_feature_id: row.feature_id,
                header: [0; 2],
                root_schema_class: row.root_schema_class.map(SchemaClass::code),
                stream_offset: row.stream_offset,
                body: &row.body,
                body_offset: row.body_offset,
                offset: row.offset,
                source_section: source_section_ref(ctx, scan, row.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_choice_field_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureChoiceFieldRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for field in ctx.admit_iter(&scan.features.choice_fields, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:feature:choice_field#{}", field.offset),
                "creo native feature choice field record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native feature choice field records")?;
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
                        body,
                        decoded_values: decoded_values.as_deref(),
                    },
                    crate::feature::rows::FeatureFieldValue::Raw(bytes) => {
                        CreoFeatureFieldValue::Raw { bytes }
                    }
                },
                offset: field.offset,
                source_section: source_section_ref(ctx, scan, field.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn half_edge_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoHalfEdgeRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        if scan.topology.half_edges.is_empty() {
            return Ok(Vec::new());
        }
        let mut topology_storage =
            ctx.reserve_scoped(0, "creo native topology row index storage")?;
        let mut topology_rows = HashMap::new();
        for row in ctx.admit_iter(&scan.curves.topology_rows, "creo native record traversal")? {
            topology_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut topology_rows,
                    row.id,
                    row,
                    "creo native half edge topology row nodes",
                )
            })?;
        }
        let mut records = Vec::new();
        for edge in ctx.admit_iter(&scan.topology.half_edges, "creo native record traversal")? {
            let Some(row) = topology_rows.get(&edge.id.curve_id) else {
                continue;
            };
            let id = ctx.format_retained(
                format_args!(
                    "creo:topology:half_edge#{}:{}",
                    edge.id.curve_id, edge.id.side
                ),
                "creo native half edge record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native half edge records")?;
            records.push(CreoHalfEdgeRecord {
                id,
                curve_id: edge.id.curve_id,
                side: edge.id.side,
                face_id: edge.face_id.map_or(0, std::num::NonZeroU32::get),
                next: edge.next.map(half_edge_ref),
                offset: row.offset,
                source_section: source_section_ref(ctx, scan, row.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn loop_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoLoopRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for (index, record) in ctx
            .admit_iter(&scan.topology.loops, "creo native record traversal")?
            .enumerate()
        {
            let id = ctx.format_retained(
                format_args!("creo:topology:loop#{}", index + 1),
                "creo native loop record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native loop records")?;
            records.push(CreoLoopRecord {
                id,
                face_id: record.face_id().map_or(0, std::num::NonZeroU32::get),
                half_edges: record.half_edges(),
            });
        }
        Ok(records)
    })
}

pub(super) fn loop_array_frame_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoLoopArrayFrameRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        if scan.loop_arrays.frames.is_empty() {
            return Ok(Vec::new());
        }
        let mut count_storage = ctx.reserve_scoped(0, "creo native loop frame count storage")?;
        let mut counts = HashMap::<usize, usize>::new();
        for record in ctx.admit_iter(&scan.loop_arrays.records, "creo native record traversal")? {
            let count = count_storage
                .with_storage(|| {
                    ctx.entry_hash_map(
                        &mut counts,
                        record.frame_offset,
                        "creo native loop array frame count nodes",
                    )
                })?
                .or_default();
            *count = count.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("creo native loop array frame counts", u64::MAX, u64::MAX)
            })?;
        }
        let mut records = Vec::new();
        for frame in ctx.admit_iter(&scan.loop_arrays.frames, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:loop_array:frame#{}", frame.offset),
                "creo native loop array frame record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native loop array frame records")?;
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
                source_section: source_section_ref(ctx, scan, frame.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn loop_array_record_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoLoopArrayRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(&scan.loop_arrays.records, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:loop_array:record#{}", record.offset),
                "creo native loop array record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native loop array records")?;
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
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn topological_vertex_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoTopologicalVertexRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(&scan.topology.vertices, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:topology:vertex#{}", record.id),
                "creo native topological vertex record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native topological vertex records")?;
            records.push(CreoTopologicalVertexRecord {
                id,
                vertex_id: record.id.get(),
                half_edges: record.half_edges(),
            });
        }
        Ok(records)
    })
}

pub(super) fn half_edge_vertex_incidence_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<CreoHalfEdgeVertexIncidenceRecord>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.topology.half_edge_vertex_incidence,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:topology:half_edge_vertex_incidence#{}:{}",
                    record.half_edge.curve_id, record.half_edge.side
                ),
                "creo native half edge vertex incidence record id",
            )?;
            ctx.reserve_vec(
                &mut records,
                1,
                "creo native half edge vertex incidence records",
            )?;
            records.push(CreoHalfEdgeVertexIncidenceRecord {
                id,
                half_edge: half_edge_ref(record.half_edge),
                start_vertex_id: record.start_vertex_id.get(),
                end_vertex_id: record.end_vertex_id.map(std::num::NonZeroU32::get),
            });
        }
        Ok(records)
    })
}

pub(super) fn face_component_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFaceComponentRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for (index, record) in ctx
            .admit_iter(
                &scan.topology.face_components,
                "creo native record traversal",
            )?
            .enumerate()
        {
            let id = ctx.format_retained(
                format_args!("creo:topology:face_component#{}", index + 1),
                "creo native face component record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native face component records")?;
            records.push(CreoFaceComponentRecord {
                id,
                face_ids: record.face_ids(),
                curve_ids: record.curve_ids(),
            });
        }
        Ok(records)
    })
}

pub(super) fn fc_curve_coordinate_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFcCurveCoordinateRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(&scan.curves.fc_coordinates, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:curve:fc_coordinates#{}", record.curve_id),
                "creo native FC curve coordinate record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native FC curve coordinate records")?;
            records.push(CreoFcCurveCoordinateRecord {
                id,
                curve_id: record.curve_id,
                subtype: record.subtype,
                body: &record.body,
                values_mm: &record.values_mm,
                tokens: &record.tokens,
                opaque_spans: &record.opaque_spans,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn prototype_pcurve_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoPrototypePcurveRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.curves.prototype_pcurves,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:curve:prototype_pcurve#{}", record.curve_id),
                "creo native prototype pcurve record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native prototype pcurve records")?;
            records.push(CreoPrototypePcurveRecord {
                id,
                curve_id: record.curve_id,
                face_0_endpoints: record.face_0_endpoints,
                face_1_endpoints: record.face_1_endpoints,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn curve_prototype_topology_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoCurvePrototypeTopologyRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.curves.prototype_topology,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:curve:prototype_topology#{}", record.curve_id),
                "creo native curve prototype topology record id",
            )?;
            ctx.reserve_vec(
                &mut records,
                1,
                "creo native curve prototype topology records",
            )?;
            records.push(CreoCurvePrototypeTopologyRecord {
                id,
                curve_id: record.curve_id,
                faces: record.stored_face_ids(),
                next_edges: record.next_edges,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn curve_prototype_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    prototypes: &'a [crate::curve::CurvePrototype],
    id_prefix: &str,
) -> Result<
    (
        Vec<CreoCurvePrototypeRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(prototypes, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("{id_prefix}#{}:{}", record.offset, record.id),
                "creo native curve prototype record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native curve prototype records")?;
            records.push(CreoCurvePrototypeRecord {
                id,
                curve_id: record.id,
                type_byte: record.type_byte,
                generating_feature_id: record.feature_id,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn plane_local_system_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    systems: &'a [crate::surface::PlaneLocalSystem],
    id_prefix: &str,
) -> Result<
    (
        Vec<CreoPlaneLocalSystemRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(systems, "creo native record traversal")? {
            let frame = record.frame();
            let id = ctx.format_retained(
                format_args!("{id_prefix}#{}:{}", record.offset, record.surface_id),
                "creo native plane local system record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native plane local system records")?;
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
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn plane_envelope_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    envelopes: &'a [crate::surface::PlaneEnvelopeRecord],
    id_prefix: &str,
) -> Result<
    (
        Vec<CreoPlaneEnvelopeRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(envelopes, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("{id_prefix}#{}:{}", record.offset, record.surface_id),
                "creo native plane envelope record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native plane envelope records")?;
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
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn outline_plane_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    planes: &'a [crate::surface::OutlinePlane],
    id_prefix: &str,
) -> Result<
    (
        Vec<CreoOutlinePlaneRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(planes, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("{id_prefix}#{}:{}", record.offset, record.surface_id),
                "creo native outline plane record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native outline plane records")?;
            records.push(CreoOutlinePlaneRecord {
                id,
                surface_id: record.surface_id,
                origin: record.origin,
                normal: record.normal(),
                u_axis: record.u_axis(),
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn datum_plane_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoDatumPlaneRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(&scan.planes.datums, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:datum:plane#{}:{}",
                    record.offset_in_payload, record.id
                ),
                "creo native datum plane record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native datum plane records")?;
            records.push(CreoDatumPlaneRecord {
                id,
                datum_id: record.id,
                owner_feature_id: record.feature_id,
                normal: record.plane().normal(),
                plane_offset: record.plane().offset(),
                corners: record.corners(),
                offset: record.offset_in_payload,
                source_section: source_section_ref(ctx, scan, record.offset_in_payload)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn datum_cylinder_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoDatumCylinderRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in
            ctx.admit_iter(&scan.planes.datum_cylinders, "creo native record traversal")?
        {
            let id = ctx.format_retained(
                format_args!(
                    "creo:datum:cylinder#{}:{}",
                    record.offset_in_payload, record.id
                ),
                "creo native datum cylinder record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native datum cylinder records")?;
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
                source_section: source_section_ref(ctx, scan, record.offset_in_payload)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_section_transform_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureSectionTransformRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut identity_storage =
            ctx.reserve_scoped(0, "creo section transform identity storage")?;
        let mut identities = HashSet::new();
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.features.section_transforms,
            "creo native record traversal",
        )? {
            if !identity_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut identities,
                    (record.definition_id, record.offset),
                    "creo section transform identities",
                )
            })? {
                continue;
            }
            let id = ctx.format_retained(
                format_args!(
                    "creo:feature:section_transform#{}:{}",
                    record.definition_id, record.offset
                ),
                "creo native section transform record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native section transform records")?;
            records.push(CreoFeatureSectionTransformRecord {
                id,
                definition_id: record.definition_id,
                owner_feature_id: record.feature_id,
                origin: record.origin(),
                u_axis: record.u_axis(),
                v_axis: record.v_axis(),
                normal: record.normal(),
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        ctx.stable_sort_by(
            records.as_mut_slice(),
            |value| &value.id,
            Ord::cmp,
            "creo feature section transform records records ordering",
        )?;
        Ok(records)
    })
}

pub(super) fn feature_placement_instruction_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeaturePlacementInstructionRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for definition in
            ctx.admit_iter(&scan.features.definitions, "creo native record traversal")?
        {
            let mut instructions =
                crate::feature::definitions::placement_instructions(ctx, definition)?;
            while let Some(instruction) = instructions.next(ctx)? {
                let id = ctx.format_retained(
                    format_args!(
                        "creo:featdefs:placement_instruction#{}:{}",
                        definition.identity.id(),
                        instruction.offset
                    ),
                    "creo native placement instruction record id",
                )?;
                ctx.reserve_vec(&mut records, 1, "creo native placement instruction records")?;
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
                    source_section: source_section_ref(ctx, scan, instruction.offset)?,
                });
            }
        }
        Ok(records)
    })
}

#[derive(Serialize)]
pub(super) struct CreoSurfaceParameterRecord<'a> {
    pub(super) id: String,
    surface_id: u32,
    surface_type_byte: u8,
    surface_family: &'static str,
    boundary: &'static str,
    body: &'a [u8],
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
    body: &'a [u8],
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
    pub(super) body: &'a [u8],
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
    Present(&'a [Vec<u8>]),
    Missing(usize),
}

impl Serialize for ScalarTokens<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Empty => serializer.collect_seq(std::iter::empty::<&[u8]>()),
            Self::Present(tokens) => serializer.collect_seq(tokens.iter()),
            Self::Missing(count) => {
                let empty: &[u8] = &[];
                serializer.collect_seq(std::iter::repeat_n(empty, *count))
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
    map.serialize_entry("opaque", &opaque)?;
    map.end()
}

#[derive(Serialize)]
pub(super) struct CreoCurveParameterRecord<'a> {
    pub(super) id: String,
    curve_id: u32,
    type_byte: u8,
    body: &'a [u8],
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
    directions: [u8; 2],
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
    directions: [u8; 2],
    suffix: crate::curve::DepdbCurveSuffix,
    body: &'a [u8],
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
    body: &'a [u8],
    surface_id: u32,
    curve_id: u32,
    curve_type: u8,
    flip: u8,
    tangent_condition: u8,
    degree: u8,
    parameter_body: &'a [u8],
    control_point_ids: [u32; 4],
    successor_reference: u32,
    control_point_bodies: &'a [Vec<u8>; 4],
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
        record.serialize_field("raw", &self.0.raw)?;
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
        record.serialize_field("raw", &self.0.raw)?;
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

pub(super) fn surface_row_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    rows: &'a [crate::surface::SurfaceRow],
    namespace: &str,
) -> Result<
    (
        Vec<CreoSurfaceRowRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for row in ctx.admit_iter(rows, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:{namespace}:surface_row#{}", row.id),
                "creo native surface row record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native surface row records")?;
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
                source_section: source_section_ref(ctx, scan, row.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn surface_prototype_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    prototypes: &'a [crate::surface::SurfacePrototypeRecord],
    id_namespace: &str,
) -> Result<
    (
        Vec<CreoSurfacePrototypeRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        use crate::surface::SurfacePrototypeFamily;
        let mut records = Vec::new();
        for record in ctx.admit_iter(prototypes, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:{id_namespace}:surface_prototype#{}", record.offset),
                "creo native surface prototype record id",
            )?;
            let family = match &record.family {
                SurfacePrototypeFamily::Plane => std::borrow::Cow::Borrowed("plane"),
                SurfacePrototypeFamily::Cylinder => std::borrow::Cow::Borrowed("cylinder"),
                SurfacePrototypeFamily::Cone => std::borrow::Cow::Borrowed("cone"),
                SurfacePrototypeFamily::Torus(_) => std::borrow::Cow::Borrowed("torus_or_sphere"),
                SurfacePrototypeFamily::Spline(_) => std::borrow::Cow::Borrowed("spline"),
                SurfacePrototypeFamily::Fillet(_) => std::borrow::Cow::Borrowed("fillet"),
                SurfacePrototypeFamily::Extrusion(_) => std::borrow::Cow::Borrowed("extrusion"),
                SurfacePrototypeFamily::Other(name) => {
                    std::borrow::Cow::Owned(ctx.format_retained(
                        format_args!("other:{name}"),
                        "creo native surface prototype family",
                    )?)
                }
            };
            let mut parameters = Vec::new();
            for parameter in ctx.admit_iter(&record.parameters, "creo native record traversal")? {
                ctx.reserve_vec(
                    &mut parameters,
                    1,
                    "creo native surface prototype parameters",
                )?;
                parameters.push(CreoSurfaceNamedParameterRecord {
                    name: &parameter.name,
                    value: &parameter.value,
                    body: &parameter.body,
                    offset: parameter.offset,
                    value_offset: parameter.value_offset,
                });
            }
            ctx.reserve_vec(&mut records, 1, "creo native surface prototype records")?;
            records.push(CreoSurfacePrototypeRecord {
                id,
                declared_family: record.family.name(),
                family,
                parameters,
                offset: record.offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn surface_contour_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    contours: &'a [crate::surface::SurfaceContourRecord],
    namespace: &str,
) -> Result<
    (
        Vec<CreoSurfaceContourRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(contours, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:{namespace}:surface_contour#{}-{}",
                    record.surface_id, record.offset
                ),
                "creo native surface contour record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native surface contour records")?;
            records.push(CreoSurfaceContourRecord {
                id,
                surface_id: record.surface_id,
                chain_index: record.chain_index,
                curve_header_id: record.curve_header_id,
                trv: record.trv,
                parameter_envelope: record.parameter_envelope,
                separator_reference: record.separator_reference,
                body: &record.body,
                offset: record.offset,
                envelope_offset: record.envelope_offset,
                surface_row_offset: record.surface_row_offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
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

fn curve_id_counts<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ids: impl IntoIterator<Item = u32>,
    operation: &'static str,
) -> Result<
    (
        HashMap<u32, usize>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut counts = HashMap::<u32, usize>::new();
    for id in ids {
        let count = storage
            .with_storage(|| ctx.entry_hash_map(&mut counts, id, operation))?
            .or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    }
    Ok((counts, storage))
}

pub(super) fn curve_parameter_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    parameters: &'a [crate::curve::CurveParameterRecord],
    id_namespace: &str,
) -> Result<
    (
        Vec<CreoCurveParameterRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let counts_parts = curve_id_counts(
            ctx,
            ctx.admit_iter(parameters, "creo curve parameter count traversal")?
                .map(|record| record.curve_id),
            "creo native curve parameter count nodes",
        )?;
        let _count_storage = counts_parts.1;
        let counts = counts_parts.0;
        let mut records = Vec::new();
        for record in ctx.admit_iter(parameters, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:{id_namespace}:curve_parameter#{}",
                    CurveOccurrenceIdentity {
                        curve_id: record.curve_id,
                        offset: record.offset,
                        occurrence_count: counts[&record.curve_id],
                    }
                ),
                "creo native curve parameter record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native curve parameter records")?;
            records.push(CreoCurveParameterRecord {
                id,
                curve_id: record.curve_id,
                type_byte: record.type_byte,
                body: &record.body,
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
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn cross_section_curve_row_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoCrossSectionCurveRowRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let counts_parts = curve_id_counts(
            ctx,
            ctx.admit_iter(
                &scan.curves.cross_section_rows,
                "creo cross section curve count traversal",
            )?
            .map(|row| row.id),
            "creo native cross section curve count nodes",
        )?;
        let _count_storage = counts_parts.1;
        let counts = counts_parts.0;
        let mut records = Vec::new();
        for row in ctx.admit_iter(
            &scan.curves.cross_section_rows,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:cross_section_geometry:curve_row#{}",
                    CurveOccurrenceIdentity {
                        curve_id: row.id,
                        offset: row.offset,
                        occurrence_count: counts[&row.id],
                    }
                ),
                "creo native cross section curve record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native cross section curve records")?;
            records.push(CreoCrossSectionCurveRowRecord {
                id,
                curve_id: row.id,
                type_byte: row.type_byte,
                feature_id: row.feature_id,
                directions: row.directions,
                suffix: row.suffix,
                body: &row.body,
                scalar_values: &row.scalar_tokens,
                scalar_tokens: &row.scalar_tokens,
                references: &row.references,
                opaque_spans: &row.opaque_spans,
                offset: row.offset,
                source_section: source_section_ref(ctx, scan, row.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn curve_topology_row_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    rows: &'a [crate::curve::CurveTopologyRow],
    id_namespace: &str,
) -> Result<
    (
        Vec<CreoCurveTopologyRowRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let counts_parts = curve_id_counts(
            ctx,
            ctx.admit_iter(rows, "creo curve topology count traversal")?
                .map(|row| row.id),
            "creo native curve topology count nodes",
        )?;
        let _count_storage = counts_parts.1;
        let counts = counts_parts.0;
        let mut records = Vec::new();
        for row in ctx.admit_iter(rows, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:{id_namespace}:curve_topology#{}",
                    CurveOccurrenceIdentity {
                        curve_id: row.id,
                        offset: row.offset,
                        occurrence_count: counts[&row.id],
                    }
                ),
                "creo native curve topology record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native curve topology records")?;
            records.push(CreoCurveTopologyRowRecord {
                id,
                curve_id: row.id,
                type_byte: row.type_byte,
                feature_id: row.feature_id,
                directions: row.directions,
                faces: row.stored_face_ids(),
                next_edges: row.next_edges,
                offset: row.offset,
                source_section: source_section_ref(ctx, scan, row.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn tabulated_cylinder_curve_replay_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoTabulatedCylinderCurveReplayRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.curves.tabulated_cylinder_replays,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:visibgeom:tabulated_cylinder_curve_replay#{}",
                    record.surface_id
                ),
                "creo native tabulated cylinder replay record id",
            )?;
            ctx.reserve_vec(
                &mut records,
                1,
                "creo native tabulated cylinder replay records",
            )?;
            records.push(CreoTabulatedCylinderCurveReplayRecord {
                id,
                body: &record.body,
                surface_id: record.surface_id,
                curve_id: record.curve_id,
                curve_type: record.curve_type,
                flip: record.flip,
                tangent_condition: record.tangent_condition,
                degree: record.degree,
                parameter_body: &record.parameter_body,
                control_point_ids: record.control_point_ids,
                successor_reference: record.successor_reference,
                control_point_bodies: &record.control_point_bodies,
                control_points: record.control_points,
                terminal_reference: record.terminal_reference,
                offset: record.offset,
                surface_row_offset: record.surface_row_offset,
                source_section: source_section_ref(ctx, scan, record.offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn surface_parameter_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
    rows: &'a crate::surface::SurfaceRows,
    parameters: &'a [crate::surface::SurfaceParameterRecord],
    namespace: &str,
) -> Result<
    (
        Vec<CreoSurfaceParameterRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(parameters, "creo native record traversal")? {
            let Some(row) = rows.unique(record.surface_id) else {
                continue;
            };
            let surface_family = surface_family(row.kind);
            let boundary = match record.boundary {
                crate::surface::SurfaceBodyBoundary::CompoundClose => "compound_close",
                crate::surface::SurfaceBodyBoundary::NextRow => "next_row",
                crate::surface::SurfaceBodyBoundary::NamedRecord => "named_record",
                crate::surface::SurfaceBodyBoundary::SectionEnd => "section_end",
            };
            let id = ctx.format_retained(
                format_args!("creo:{namespace}:surface_parameter#{}", record.surface_id),
                "creo native surface parameter record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native surface parameter records")?;
            records.push(CreoSurfaceParameterRecord {
                id,
                surface_id: record.surface_id,
                surface_type_byte: row.kind.canonical_type_byte(),
                surface_family,
                boundary,
                body: &record.body,
                slots: &record.scalar_tokens,
                opaque_spans: &record.opaque_spans,
                scalar_frames: &record.scalar_frames,
                terminal_scalar_frame: record.terminal_scalar_frame(),
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
                replayed_torus_minor_radius: replayed_torus_minor_radius(ctx, scan, row, record)?,
                cone_half_angle_override: record.cone_half_angle_override().map(|half_angle| {
                    CreoConeHalfAngleOverride {
                        radians: half_angle.radians.get().get(),
                        offset: half_angle.offset,
                    }
                }),
                extrusion_direction: record.extrusion_direction(),
                row_offset: record.offset,
                body_offset: record.body_offset,
                source_section: source_section_ref(ctx, scan, record.body_offset)?,
            });
        }
        Ok(records)
    })
}

pub(super) fn feature_operation_state_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
) -> Result<
    (
        Vec<CreoFeatureOperationState<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        if scan.features.operation_states.is_empty() {
            return Ok(Vec::new());
        }
        let mut state_storage = ctx.reserve_scoped(0, "Creo operation state lookup storage")?;
        let mut current_offsets = HashMap::new();
        for state in ctx.admit_iter(&scan.features.operations, "creo native record traversal")? {
            state_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut current_offsets,
                    state.feature_id,
                    state.offset,
                    "creo native feature current-offset nodes",
                )
            })?;
        }
        let mut ordinals = HashMap::<u32, usize>::new();
        let mut records = Vec::new();
        for state in ctx.admit_iter(
            &scan.features.operation_states,
            "creo native record traversal",
        )? {
            let state_ordinal = ordinals.get(&state.feature_id).copied().unwrap_or_default();
            let next_ordinal = state_ordinal.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("creo native feature state ordinal", u64::MAX, u64::MAX)
            })?;
            state_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut ordinals,
                    state.feature_id,
                    next_ordinal,
                    "creo native feature ordinal nodes",
                )
            })?;
            let name = CreoOperationNameRecord {
                display_name_stored: state.name.display_name_stored(),
                stored_name: state
                    .name
                    .stored_name_bytes()
                    .map(|bytes| {
                        ctx.copy_retained_lossy_utf8(bytes, "creo native feature state name")
                    })
                    .transpose()?,
                stored_name_bytes: state.name.stored_name_bytes(),
                identifier_keyword: state.name.identifier_keyword(),
                stored_name_prefix: state
                    .name
                    .stored_name_prefix()
                    .map(|prefix| {
                        ctx.format_retained(
                            format_args!("{}", char::from(prefix)),
                            "creo native feature state prefix",
                        )
                    })
                    .transpose()?,
            };
            let id = ctx.format_retained(
                format_args!(
                    "creo:mdlstatus:feature_state#{}:{state_ordinal}",
                    state.feature_id
                ),
                "creo native feature state IDs",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native feature state records")?;
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
    })
}

pub(super) fn feature_reference_name_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureReferenceNameRecord>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(
            &scan.features.reference_names,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!("creo:mdlrefinfo:feature_name#{}", record.offset),
                "creo native feature reference IDs",
            )?;
            let name = ctx.copy_retained_lossy_utf8(
                &record.name_bytes,
                "creo native feature reference text",
            )?;
            let name_bytes =
                ctx.copy_retained(&record.name_bytes, "creo native feature reference bytes")?;
            ctx.reserve_vec(&mut records, 1, "creo native feature reference records")?;
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
    })
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

pub(super) fn pcurve_endpoint_records<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<
    (
        Vec<(CreoPcurveEndpointRecord, usize)>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for pcurve in ctx.admit_iter(&scan.curves.pcurves, "creo native record traversal")? {
            let id = ctx.format_retained(
                format_args!("creo:visibgeom:pcurve_endpoints#{}", pcurve.curve_id),
                "creo native pcurve endpoint record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native pcurve endpoint records")?;
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
        for pcurve in ctx.admit_iter(
            &scan.curves.bound_prototype_pcurves,
            "creo native record traversal",
        )? {
            let id = ctx.format_retained(
                format_args!(
                    "creo:visibgeom:prototype_pcurve_endpoints#{}",
                    pcurve.curve_id
                ),
                "creo native prototype pcurve endpoint record id",
            )?;
            ctx.reserve_vec(&mut records, 1, "creo native pcurve endpoint records")?;
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
        ctx.stable_sort_by(
            records.as_mut_slice(),
            |value| &value.1,
            Ord::cmp,
            "creo pcurve endpoint records records ordering",
        )?;
        Ok(records)
    })
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

pub(super) fn curve_expression_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoCurveExpressionRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        let mut records = Vec::new();
        for record in ctx.admit_iter(&scan.curves.expressions, "creo native record traversal")? {
            let id = curve_expression_record_id(ctx, record)?;
            let mut lines = Vec::new();
            for line in ctx.admit_iter(&record.lines, "creo native record traversal")? {
                ctx.reserve_vec(&mut lines, 1, "creo native curve expression lines")?;
                lines.push(CreoCurveExpressionLine {
                    text: &line.text,
                    offset: line.offset,
                });
            }
            let mut assignments = Vec::new();
            for assignment in ctx.admit_iter(&record.assignments, "creo native record traversal")? {
                ctx.reserve_vec(
                    &mut assignments,
                    1,
                    "creo native curve expression assignments",
                )?;
                assignments.push(curve_expression_assignment_projection(assignment));
            }
            let mut solve_blocks = Vec::new();
            for block in ctx.admit_iter(&record.solve_blocks, "creo native record traversal")? {
                let mut equations = Vec::new();
                for equation in ctx.admit_iter(&block.equations, "creo native record traversal")? {
                    ctx.reserve_vec(&mut equations, 1, "creo native curve expression equations")?;
                    equations.push(CreoCurveExpressionEquation {
                        left: &equation.left,
                        right: &equation.right,
                        dependencies: &equation.dependencies,
                        offset: equation.offset,
                    });
                }
                let mut block_assignments = Vec::new();
                for assignment in
                    ctx.admit_iter(&block.assignments, "creo native record traversal")?
                {
                    ctx.reserve_vec(
                        &mut block_assignments,
                        1,
                        "creo native curve expression block assignments",
                    )?;
                    block_assignments.push(curve_expression_assignment_projection(assignment));
                }
                ctx.reserve_vec(
                    &mut solve_blocks,
                    1,
                    "creo native curve expression solve blocks",
                )?;
                solve_blocks.push(CreoCurveExpressionSolveBlock {
                    equations,
                    assignments: block_assignments,
                    variables: &block.unknowns,
                    solutions: &block.unknowns,
                    offset: block.offset,
                    for_offset: block.for_offset,
                });
            }
            ctx.reserve_vec(&mut records, 1, "creo native curve expression records")?;
            records.push(CreoCurveExpressionRecord {
                id,
                entity_id: record.entity_id,
                backup: record.backup,
                local_system: record.local_system.as_ref().map(|frame| {
                    CreoCurveExpressionLocalSystem {
                        dimensions: frame.dimensions,
                        count: frame.count,
                        body: &frame.body,
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
        Ok(records)
    })
}

pub(super) fn sketch_records<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoSketchRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    cadmpeg_core::CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "creo native sketch projection storage")?;
    let mut identity_storage = ctx.reserve_scoped(0, "creo sketch definition identity storage")?;
    let mut identity_counts = HashMap::<u32, usize>::new();
    let mut identities_indexed = false;
    let mut records = Vec::new();
    for definition in ctx.admit_iter(&scan.features.definitions, "creo native record traversal")? {
        if !feature_definition_has_sketch_design(ctx, definition)? {
            continue;
        }
        if !identities_indexed {
            for definition in ctx.admit_iter(
                &scan.features.definitions,
                "creo sketch record identity count",
            )? {
                let count = identity_storage
                    .with_storage(|| {
                        ctx.entry_hash_map(
                            &mut identity_counts,
                            definition.identity.id(),
                            "creo sketch identity count nodes",
                        )
                    })?
                    .or_default();
                *count = count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("creo sketch identity multiplicity", u64::MAX, u64::MAX)
                })?;
            }
            identities_indexed = true;
        }
        let (
            id,
            source_section,
            segments,
            circle_segments,
            point_segments,
            centered_line_segments,
            reference_line_segments,
            bounded_curve_segments,
            conic_segments,
            opaque_segments,
        ) = storage.with_storage(|| {
            let id = if identity_counts.get(&definition.identity.id()) != Some(&1)
                || (definition.identity.schema_id().is_none()
                    && definition.identity.owner_feature_id().is_none())
            {
                ctx.format_retained(
                    format_args!("creo:featdefs:sketch#offset:{}", definition.offset),
                    "creo sketch record id",
                )?
            } else {
                ctx.format_retained(
                    format_args!("creo:featdefs:sketch#{}", definition.identity.id()),
                    "creo sketch record id",
                )?
            };
            let source_section = ctx.copy_retained_text(
                source_section_ref(ctx, scan, definition.offset)?,
                "creo sketch source section",
            )?;
            let mut segments = Vec::new();
            let mut circle_segments = Vec::new();
            let mut point_segments = Vec::new();
            let mut centered_line_segments = Vec::new();
            let mut reference_line_segments = Vec::new();
            let mut bounded_curve_segments = Vec::new();
            let mut conic_segments = Vec::new();
            let mut opaque_segments = Vec::new();
            let segment_rows = definition
                .segments
                .as_ref()
                .map_or(&[][..], |table| table.rows.as_slice());
            for row in ctx.admit_iter(segment_rows, "creo sketch segment projection traversal")? {
                match row {
                    crate::feature::segment_rows::SegmentRow::Ordinary(segment) => {
                        ctx.reserve_vec(&mut segments, 1, "creo native sketch segments")?;
                        segments.push(CreoSketchSegment {
                            external_id: segment.external_id,
                            kind: match segment.kind {
                                crate::feature::definitions::FeatureSegmentKind::Line(_) => "line",
                                crate::feature::definitions::FeatureSegmentKind::Arc(_) => "arc",
                                crate::feature::definitions::FeatureSegmentKind::Point(_) => {
                                    "point"
                                }
                            },
                            point_ids: segment.point_ids(),
                            center_id: segment.center_id,
                            directions: segment.directions,
                            arc_orientation: segment.arc_orientation,
                            vertical_horizontal_constraint: segment.vertical_horizontal,
                            radius_dimension_id: segment.radius_ref,
                            secondary_radius_dimension_id: segment.radius2_ref,
                            body: ctx
                                .copy_retained(&segment.body, "creo native sketch segment body")?,
                            offset: segment.offset,
                        });
                    }
                    crate::feature::segment_rows::SegmentRow::Circle(segment) => {
                        ctx.reserve_vec(
                            &mut circle_segments,
                            1,
                            "creo native sketch circle segments",
                        )?;
                        circle_segments.push(CreoSketchCircleSegment {
                            external_id: segment.external_id,
                            center_id: segment.center_id,
                            radius_dimension_id: segment.radius_ref,
                            offset: segment.offset,
                        });
                    }
                    crate::feature::segment_rows::SegmentRow::Point(segment) => {
                        ctx.reserve_vec(
                            &mut point_segments,
                            1,
                            "creo native sketch point segments",
                        )?;
                        point_segments.push(CreoSketchPointSegment {
                            external_id: segment.external_id,
                            point_id: segment.point_id,
                            offset: segment.offset,
                        });
                    }
                    crate::feature::segment_rows::SegmentRow::CenteredLine(segment) => {
                        ctx.reserve_vec(
                            &mut centered_line_segments,
                            1,
                            "creo native sketch centered line segments",
                        )?;
                        centered_line_segments.push(CreoSketchCenteredLineSegment {
                            external_id: segment.external_id,
                            center_id: segment.center_id,
                            offset: segment.offset,
                        });
                    }
                    crate::feature::segment_rows::SegmentRow::ReferenceLine(segment) => {
                        ctx.reserve_vec(
                            &mut reference_line_segments,
                            1,
                            "creo native sketch reference line segments",
                        )?;
                        reference_line_segments.push(CreoSketchReferenceLineSegment {
                            external_id: segment.external_id,
                            point_ids: segment.point_ids,
                            directions: segment.directions,
                            vertical_horizontal_constraint: segment.vertical_horizontal,
                            offset: segment.offset,
                        });
                    }
                    crate::feature::segment_rows::SegmentRow::BoundedCurve(segment) => {
                        ctx.reserve_vec(
                            &mut bounded_curve_segments,
                            1,
                            "creo native sketch bounded curve segments",
                        )?;
                        bounded_curve_segments.push(CreoSketchBoundedCurveSegment {
                            external_id: segment.external_id,
                            point_ids: segment.point_ids,
                            center_id: segment.center_id,
                            directions: segment.directions,
                            arc_orientation: segment.arc_orientation,
                            vertical_horizontal_constraint: segment.vertical_horizontal,
                            radius_dimension_id: segment.radius_ref,
                            secondary_radius_dimension_id: segment.radius2_ref,
                            offset: segment.offset,
                        });
                    }
                    crate::feature::segment_rows::SegmentRow::Conic(segment) => {
                        ctx.reserve_vec(
                            &mut conic_segments,
                            1,
                            "creo native sketch conic segments",
                        )?;
                        conic_segments.push(CreoSketchConicSegment {
                            external_id: segment.external_id,
                            center_id: segment.center_id,
                            first_coefficient_ref: segment.first_coefficient_ref,
                            second_coefficient_ref: segment.second_coefficient_ref,
                            offset: segment.offset,
                        });
                    }
                    crate::feature::segment_rows::SegmentRow::Opaque(segment) => {
                        ctx.reserve_vec(
                            &mut opaque_segments,
                            1,
                            "creo native sketch opaque segments",
                        )?;
                        opaque_segments.push(CreoSketchOpaqueSegment {
                            external_id: segment.external_id,
                            kind: segment.kind,
                            point_ids: segment.point_ids,
                            center_id: segment.center_id,
                            directions: segment.directions,
                            arc_orientation: segment.arc_orientation,
                            vertical_horizontal_constraint: segment.vertical_horizontal,
                            radius_dimension_id: segment.radius_ref,
                            secondary_radius_dimension_id: segment.radius2_ref,
                            body: ctx.copy_retained(
                                &segment.body,
                                "creo native sketch opaque segment body",
                            )?,
                            offset: segment.offset,
                        });
                    }
                }
            }
            Ok::<_, CodecError>((
                id,
                source_section,
                segments,
                circle_segments,
                point_segments,
                centered_line_segments,
                reference_line_segments,
                bounded_curve_segments,
                conic_segments,
                opaque_segments,
            ))
        })?;
        let table_headers = storage.with_storage(|| sketch_table_headers(ctx, definition))?;
        let section_points = sketch_section_point_records(ctx, definition, &mut storage)?;
        let solved_external_ids = storage.with_storage(|| {
            ctx.collect_vec(
                ctx.admit_iter(
                    definition
                        .trim_entities
                        .as_ref()
                        .map_or(&[][..], |table| table.solved_external_ids.as_slice()),
                    "creo native solved ID traversal",
                )?
                .copied(),
                "creo native sketch solved external IDs",
            )
        })?;
        let variables = {
            let mut resolution_storage =
                ctx.reserve_scoped(0, "creo native sketch resolved variable storage")?;
            let mut resolved_coordinates = None;
            let mut resolved_radii = None;
            let mut resolved_scalars = None;
            storage.with_storage(|| {
                ctx.try_collect_vec(
                    (ctx.admit_iter(
                        definition
                            .variables
                            .as_ref()
                            .map_or(&[][..], |table| table.rows.as_slice()),
                        "creo native sketch row traversal",
                    )?)
                    .map(|row| {
                        Ok::<_, CodecError>(CreoSketchVariable {
                            variable_type: row.variable_type.code(),
                            key: row.key,
                            value: row.value,
                            value_body: ctx.copy_retained(
                                &row.value_body,
                                "creo native sketch variable value body",
                            )?,
                            guess: row.guess,
                            guess_body: ctx.copy_retained(
                                &row.guess_body,
                                "creo native sketch variable guess body",
                            )?,
                            known: row.known,
                            homogeneity: row.homogeneity,
                            uvar_id: row.uvar_id,
                            resolved_value: match row.variable_type {
                                VariableType::U | VariableType::V => {
                                    let coordinates = match &mut resolved_coordinates {
                                        Some(coordinates) => coordinates,
                                        slot => slot.insert(resolution_storage.with_storage(|| {
                                            resolved_section_coordinates(ctx, definition)
                                        })?),
                                    };
                                    let coordinate = if row.variable_type == VariableType::U {
                                        0
                                    } else {
                                        1
                                    };
                                    ctx.get_btree_map(
                                        coordinates,
                                        &row.key,
                                        "creo sketch coordinate lookup",
                                    )?
                                    .and_then(|point| point[coordinate])
                                }
                                VariableType::Radius => {
                                    let radii = match &mut resolved_radii {
                                        Some(radii) => radii,
                                        slot => slot.insert(resolution_storage.with_storage(|| {
                                            resolved_section_radii(ctx, definition)
                                        })?),
                                    };
                                    ctx.get_btree_map(
                                        radii,
                                        &row.key,
                                        "creo sketch radius lookup",
                                    )?
                                    .copied()
                                }
                                _ => {
                                    let scalars = match &mut resolved_scalars {
                                        Some(scalars) => scalars,
                                        slot => slot.insert(resolution_storage.with_storage(|| {
                                            resolved_section_scalar_values(ctx, definition)
                                        })?),
                                    };
                                    ctx.get_btree_map(
                                        scalars,
                                        &(row.variable_type, row.key),
                                        "creo sketch scalar lookup",
                                    )?
                                    .copied()
                                }
                            },
                            offset: row.offset,
                        })
                    }),
                    "creo native sketch variables",
                )
            })?
        };
        let equations = {
            let table = crate::feature::definitions::equation_table(
                ctx,
                &definition.body,
                0,
                definition.body.len(),
            )?;
            let rows = table.map_or_else(Vec::new, |table| table.rows);
            let _rows = ctx.admit_iter(&rows, "creo native sketch equation traversal")?;
            storage.with_storage(|| {
                ctx.try_collect_vec(
                    rows.into_iter().map(|equation| {
                        Ok::<_, CodecError>(CreoSketchEquation {
                            equation_id: equation.equation_id,
                            function_id: equation.function_id,
                            explicit_argument_count: equation.explicit_argument_count,
                            arguments: equation.arguments,
                            arguments_body: equation.arguments_body,
                            auxiliary_body: equation.auxiliary_body,
                            body: equation.body,
                            offset: definition.body_position(equation.offset)?.source()?.get(),
                        })
                    }),
                    "creo native sketch equations",
                )
            })?
        };
        let record = storage.with_storage(|| Ok::<_, CodecError>(CreoSketchRecord {
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
            table_headers,
            section_points,
            solved_external_ids,
            variables,
            equations,
            segments,
            circle_segments,
            point_segments,
            centered_line_segments,
            reference_line_segments,
            bounded_curve_segments,
            conic_segments,
            opaque_segments,
            trim_entities: ctx.collect_vec(
                ctx.admit_iter(definition.trim_entities.as_ref().map_or(&[][..], |table| table.rows.as_slice()), "creo native sketch row traversal")?
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
                    }),
                "creo native sketch trim entities",
            )?,
            trim_vertices: ctx.try_collect_vec(
                (ctx.admit_iter(definition.trim_vertices.as_ref().map_or(&[][..], |table| table.rows.as_slice()), "creo native sketch row traversal")?)
                .map(|vertex| {
                    Ok::<_, CodecError>(CreoSketchTrimVertex {
                        vertex_id: vertex.vertex_id,
                        entities: ctx.collect_vec(
                            ctx.admit_iter(&vertex.entities, "creo native sketch trim vertex entity traversal")?.copied(),
                            "creo native sketch trim vertex entities",
                        )?,
                        section_coordinates: vertex.section_coordinates.map(|point| {
                            let point = point.get();
                            [point.u, point.v]
                        }),
                        offset: vertex.offset,
                    })
                }),
                "creo native sketch trim vertices",
            )?,
            order_rows: ctx.collect_vec(
                ctx.admit_iter(definition.order_table.as_ref().map_or(&[][..], |table| table.rows.as_slice()), "creo native sketch row traversal")?
                    .map(|row| CreoSketchOrderRow {
                        external_id: row.external_id,
                        internal_id: row.internal_id,
                        bitmask: row.bitmask,
                        offset: row.offset,
                    }),
                "creo native sketch order rows",
            )?,
            saved_entities: ctx.collect_vec(
                ctx.admit_iter(definition.saved_section.as_ref().map_or(&[][..], |table| table.entities.as_slice()), "creo native sketch row traversal")?
                    .map(|entity| match entity {
                        crate::feature::definitions::FeatureSavedEntity::Line(line) => {
                            CreoSketchSavedEntity::Line {
                                entity_id: line.entity_id,
                                references: &line.references,
                                attributes: &line.attributes,
                                endpoints: line.endpoints,
                                body: &line.body,
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
                                body: &arc.body,
                                offset: arc.offset,
                            }
                        }
                        crate::feature::definitions::FeatureSavedEntity::Circle(circle) => {
                            CreoSketchSavedEntity::Circle {
                                entity_id: circle.entity_id,
                                center: circle.center,
                                radius: circle.radius,
                                body: &circle.body,
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
                                body: &conic.body,
                                offset: conic.offset,
                            }
                        }
                        crate::feature::definitions::FeatureSavedEntity::Spline(spline) => {
                            CreoSketchSavedEntity::Spline {
                                entity_id: spline.entity_id,
                                declared_point_count: spline.declared_point_count,
                                interpolation_points: &spline.interpolation_points,
                                interpolation_points_body: &spline.interpolation_points_body,
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
                                body: &dummy.body,
                                offset: dummy.offset,
                            }
                        }
                    }),
                "creo native sketch saved entities",
            )?,
            dimensions: ctx.try_collect_vec(
                (ctx.admit_iter(definition.dimensions.as_ref().map_or(&[][..], |table| table.rows.as_slice()), "creo native sketch row traversal")?).map(|dimension| {
                    Ok::<_, CodecError>(CreoSketchDimension {
                        external_id: dimension.external_id,
                        dimension_type: dimension.dimension_type,
                        value: match &dimension.value {
                            crate::feature::definitions::DimensionValue::Resolved(value) => {
                                crate::feature::definitions::DimensionValue::Resolved(*value)
                            }
                            crate::feature::definitions::DimensionValue::UnresolvedToken(token) => {
                                crate::feature::definitions::DimensionValue::UnresolvedToken(
                                    ctx.copy_retained(
                                        token,
                                        "creo native sketch dimension unresolved token",
                                    )?,
                                )
                            }
                            crate::feature::definitions::DimensionValue::Undefined => {
                                crate::feature::definitions::DimensionValue::Undefined
                            }
                        },
                        value_body: ctx.copy_retained(
                            &dimension.value_body,
                            "creo native sketch dimension value body",
                        )?,
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
                        auxiliary_body: ctx.copy_retained(
                            &dimension.auxiliary_body,
                            "creo native sketch dimension auxiliary body",
                        )?,
                        references: dimension
                            .references
                            .as_ref()
                            .map(
                                |table| -> Result<CreoSketchDimensionReferenceTable, CodecError> {
                                    Ok::<_, CodecError>(CreoSketchDimensionReferenceTable {
                                        declared_count: table.declared_count,
                                        entity_ref: table.entity_ref,
                                        rows: ctx.collect_vec(
                                            ctx.admit_iter(&table.rows, "creo native sketch dimension reference traversal")?.map(|reference| {
                                                CreoSketchDimensionReference {
                                                    item_id: reference.item_id,
                                                    sense: reference.sense,
                                                    point: reference.point,
                                                    offset: reference.offset,
                                                }
                                            }),
                                            "creo native sketch dimension references",
                                        )?,
                                        offset: table.offset,
                                    })
                                },
                            )
                            .transpose()?,
                        offset: dimension.offset,
                    })
                }),
                "creo native sketch dimensions",
            )?,
            relations: ctx.try_collect_vec(
                (ctx.admit_iter(definition.relations.as_ref().map_or(&[][..], |table| table.rows.as_slice()), "creo native sketch row traversal")?).map(|relation| {
                    Ok::<_, CodecError>(CreoSketchRelation {
                        relation_id: relation.relation_id,
                        used: relation.used,
                        operands: ctx.copy_retained(
                            &relation.operands,
                            "creo native sketch relation operands",
                        )?,
                        operand_vectors: relation.operand_vectors,
                        sign: relation.sign,
                        dimension_id: relation.dimension_id,
                        relation_type: relation.relation_type,
                        body: ctx
                            .copy_retained(&relation.body, "creo native sketch relation body")?,
                        offset: relation.offset,
                    })
                }),
                "creo native sketch relations",
            )?,
            skamps: ctx.try_collect_vec(
                (ctx.admit_iter(definition.relations.as_ref().map_or(&[][..], FeatureRelationTable::skamps), "creo native sketch skamp traversal")?)
                .map(|skamp| {
                    Ok::<_, CodecError>(CreoSketchSkamp {
                        id: skamp.id,
                        kind: skamp.kind,
                        flags: skamp.flags,
                        status: skamp.status,
                        items: ctx.collect_vec(
                            ctx.admit_iter(&skamp.items, "creo native sketch skamp item traversal")?.map(|item| CreoSketchSkampItem {
                                entity_id: item.entity_id,
                                sense: item.sense,
                            }),
                            "creo native sketch skamp items",
                        )?,
                        offset: skamp.offset,
                    })
                }),
                "creo native sketch skamps",
            )?,
            relation_triples: ctx.collect_vec(
                ctx.admit_iter(definition.relations.as_ref().map_or(&[][..], FeatureRelationTable::triples), "creo native sketch triple traversal")?
                    .map(|triple| CreoSketchRelationTriple {
                        relation: triple.relation_id,
                        equation: triple.equation_id,
                        skamp: triple.skamp_id,
                        offset: triple.offset,
                    }),
                "creo native sketch relation triples",
            )?,
        }))?;
        storage.with_storage(|| ctx.reserve_vec(&mut records, 1, "creo sketch records"))?;
        records.push(record);
    }
    Ok((records, storage))
}

pub(super) fn sketch_section_point_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<Vec<CreoSketchSectionPoint>, cadmpeg_core::CodecError> {
    let Some(variables) = &definition.variables else {
        return Ok(Vec::new());
    };
    let mut point_storage = ctx.reserve_scoped(0, "creo sketch point index storage")?;
    let crate::feature::definitions::ReconciledPoints { points, ambiguous } =
        variables.reconciled_points(ctx)?;
    let mut point_ids = BTreeSet::new();
    point_storage.with_storage(|| {
        for point_id in ctx
            .admit_iter(&points, "creo sketch resolved point traversal")?
            .map(|(id, _)| *id)
            .chain(
                ctx.admit_iter(&ambiguous, "creo sketch ambiguous point traversal")?
                    .copied(),
            )
        {
            ctx.insert_btree_set(
                &mut point_ids,
                point_id,
                "creo sketch section point ID nodes",
            )?;
        }
        Ok::<(), CodecError>(())
    })?;
    let ids = ctx.admit_iter(&point_ids, "creo sketch point projection traversal")?;
    storage.with_storage(|| {
        ctx.try_collect_vec(
            ids.copied().map(|point_id| {
                let [u, v] = ctx
                    .get_btree_map(&points, &point_id, "creo sketch resolved point lookup")?
                    .copied()
                    .unwrap_or([None; 2]);
                let state = if ctx.contains_btree_set(
                    &ambiguous,
                    &point_id,
                    "creo sketch ambiguous point lookup",
                )? {
                    CreoSketchPointState::Conflicting
                } else {
                    match (u, v) {
                        (Some(u), Some(v)) => CreoSketchPointState::Resolved([u, v]),
                        (Some(u), None) => CreoSketchPointState::PartialU(u),
                        (None, Some(v)) => CreoSketchPointState::PartialV(v),
                        (None, None) => CreoSketchPointState::Unresolved,
                    }
                };
                Ok::<_, CodecError>(CreoSketchSectionPoint { point_id, state })
            }),
            "creo sketch section point records",
        )
    })
}

pub(super) fn feature_definition_records<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &'a ContainerScan,
) -> Result<
    (
        Vec<CreoFeatureDefinitionRecord<'a>>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.with_scoped_storage("creo native projection storage", || {
        if scan.features.definitions.is_empty() {
            return Ok(Vec::new());
        }
        let counts_parts = curve_id_counts(
            ctx,
            ctx.admit_iter(
                &scan.features.definitions,
                "creo feature definition identity count",
            )?
            .map(|definition| definition.identity.id()),
            "creo native feature definition identity nodes",
        )?;
        let _count_storage = counts_parts.1;
        let counts = counts_parts.0;
        let mut records = Vec::new();
        for definition in
            ctx.admit_iter(&scan.features.definitions, "creo native record traversal")?
        {
            let id = if counts.get(&definition.identity.id()) != Some(&1)
                || (definition.identity.schema_id().is_none()
                    && definition.identity.owner_feature_id().is_none())
            {
                ctx.format_retained(
                    format_args!(
                        "creo:featdefs:feature_definition#offset:{}",
                        definition.offset
                    ),
                    "creo feature definition record id",
                )?
            } else {
                ctx.format_retained(
                    format_args!(
                        "creo:featdefs:feature_definition#{}",
                        definition.identity.id()
                    ),
                    "creo feature definition record id",
                )?
            };
            let mut parameter_frames = Vec::new();
            for frame in
                ctx.admit_iter(&definition.parameter_frames, "creo native record traversal")?
            {
                ctx.reserve_vec(
                    &mut parameter_frames,
                    1,
                    "creo native feature parameter frames",
                )?;
                parameter_frames.push(CreoFeatureParameterFrame {
                    kind: match frame.kind {
                        crate::feature::definitions::FeatureParameterFrameKind::LocalSystem => {
                            "local_system"
                        }
                        crate::feature::definitions::FeatureParameterFrameKind::Transform => {
                            "transform"
                        }
                    },
                    body: &frame.body,
                    decoded_values: frame
                        .decoded_values
                        .map(cadmpeg_ir::units::FiniteVector::get),
                    offset: frame.offset,
                });
            }
            let mut outlines = Vec::new();
            for outline in ctx.admit_iter(&definition.outlines, "creo native record traversal")? {
                ctx.reserve_vec(&mut outlines, 1, "creo native feature outlines")?;
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
            ctx.reserve_vec(&mut records, 1, "creo native feature definition records")?;
            records.push(CreoFeatureDefinitionRecord {
                id,
                definition_id: definition.identity.id(),
                owner_feature_id: definition.identity.owner_feature_id(),
                source_section: source_section_ref(ctx, scan, definition.offset)?,
                body: &definition.body,
                parameter_frames,
                outlines,
                offset: definition.offset,
            });
        }
        Ok(records)
    })
}

pub(super) fn family_table_record(scan: &ContainerScan) -> Option<CreoFamilyTableRecord> {
    let record = scan.framing.family_table?;
    Some(CreoFamilyTableRecord {
        pointer: record.pointer,
        offset: record.offset,
    })
}

#[cfg(test)]
mod tests;
