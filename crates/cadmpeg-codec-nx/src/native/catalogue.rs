// SPDX-License-Identifier: Apache-2.0
//! Declarative catalogue of the native record families.
//!
//! One [`CatalogueRow`] per model field (179 total). Each row names the `nx`
//! namespace arena the family serializes into, and — for families that also emit
//! source annotations — the tag, exactness, and a `note` fn. Row order is the
//! observable annotation-emission order for the note-bearing rows;
//! [`NOTE_GROUP_A_END`] / [`NOTE_GROUP_B_END`] mark the semantic-island split
//! that [`super::attach`] walks. Arena serialization order is not observable
//! (arenas live in a `BTreeMap`), so the non-noting tail rows follow the legacy
//! arena-pass order purely for readability.
//!
//! Whether a family notes into the shared `nx:container` stream or a per-record
//! `nx:s{ordinal}` stream is encoded in its `note` fn, not a row field.
//!
//! Per the IR-write firewall this module names `cadmpeg_ir` boundary types
//! (`AnnotationBuilder`, `NativeNamespace`, `Exactness`, `NativeConvertError`)
//! and calls the annotation/arena mutation surface from the row fns; the five
//! domain modules and `model.rs` carry no `cadmpeg_ir` reference.

use serde::Serialize;

use cadmpeg_ir::{AnnotationBuilder, Exactness, NativeConvertError, NativeNamespace};

use super::model::NativeModel;
#[allow(clippy::wildcard_imports)]
use super::{display_jt::*, features::*, om::*, parasolid::*, segments::*};

/// One native record family: its arena, note metadata, and the fns that
/// serialize and (optionally) annotate it.
pub(crate) struct CatalogueRow {
    /// The `nx` namespace arena name.
    pub(crate) arena: &'static str,
    /// Annotation tag for standard note rows; `None` for custom-note and
    /// arena-only rows.
    pub(crate) tag: Option<&'static str>,
    /// Entity exactness for the family's standard notes.
    pub(crate) exactness: Exactness,
    /// Emits this family's annotations, or `None` for arena-only families and
    /// families whose notes a semantic island emits.
    pub(crate) note: Option<fn(&NativeModel, &CatalogueRow, &mut AnnotationBuilder)>,
    /// Serializes this family into its arena when non-empty.
    pub(crate) emit:
        fn(&NativeModel, &CatalogueRow, &mut NativeNamespace) -> Result<(), NativeConvertError>,
    /// Record count for this family, feeding the catalogue-derived emptiness
    /// fold ([`NativeModel::is_empty`]) and inspect counts.
    pub(crate) len: fn(&NativeModel) -> usize,
    /// Whether an empty family contributes to [`NativeModel::is_empty`]. 133 of
    /// the 179 families count; the 46 that do not are transcribed verbatim from
    /// the legacy hand-written all-empty guard, which omitted them. The
    /// exclusions look like oversights (25 of the 26 `display_jt` families are
    /// excluded, for instance) but are frozen observable behavior: flipping any
    /// one changes whether a part is treated as empty and therefore its output.
    pub(crate) counts_toward_emptiness: bool,
}

/// Index one past the last group-A note row. [`super::attach`] emits notes for
/// `CATALOGUE[..NOTE_GROUP_A_END]`, then the interleaved semantic islands, then
/// the group-B notes in `CATALOGUE[NOTE_GROUP_A_END..NOTE_GROUP_B_END]`.
pub(crate) const NOTE_GROUP_A_END: usize = 80;
/// Index one past the last group-B note row; rows beyond it are arena-only or
/// island-noted (`part_attributes`, `configurations`).
pub(crate) const NOTE_GROUP_B_END: usize = 83;

/// Serialize a record family into its arena when non-empty. The single shape
/// every `emit` row shares; each row supplies its family slice and arena name.
fn emit_arena<T: Serialize>(
    records: &[T],
    catalogue_row: &CatalogueRow,
    ns: &mut NativeNamespace,
) -> Result<(), NativeConvertError> {
    if !records.is_empty() {
        ns.set_arena(catalogue_row.arena, records)?;
    }
    Ok(())
}

/// A record noted into the shared `nx:container` stream: its id and the source
/// offset the note points at.
trait ContainerNoted {
    fn container_note(&self) -> (&str, u64);
}

/// A record noted into its own `nx:s{ordinal}` stream: its id, the stream
/// ordinal, and the inflated-byte offset the note points at.
trait StreamNoted {
    fn stream_note(&self) -> (&str, u32, u64);
}

/// Emit the standard container-stream note for every record in a family: one
/// `nx:container` note at the record's source offset tagged with the row's tag,
/// plus the row's exactness.
fn note_container<T: ContainerNoted>(
    records: &[T],
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    let stream = a.stream("nx:container");
    for record in records {
        let (id, offset) = record.container_note();
        a.note(id, stream, offset).tag(tag);
        a.exactness(id, catalogue_row.exactness);
    }
}

/// Emit the standard per-stream note for every record in a family: one note in
/// the record's own `nx:s{ordinal}` stream at its inflated offset tagged with
/// the row's tag, plus the row's exactness.
fn note_per_stream<T: StreamNoted>(
    records: &[T],
    catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let tag = catalogue_row.tag.expect("standard note row carries a tag");
    for record in records {
        let (id, stream_ordinal, offset) = record.stream_note();
        let stream = a.stream(format!("nx:s{stream_ordinal}"));
        a.note(id, stream, offset).tag(tag);
        a.exactness(id, catalogue_row.exactness);
    }
}

impl ContainerNoted for DisplayJtSegment {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtShapeLodElement {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtTriStripLodHeader {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtInitialFaceDegreeSymbols {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtTopologyPacketSequence {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtCompressedVertexRecordsHeader {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexCoordinateArrayHeader {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexCoordinates {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexNormals {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexColors {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexTextureCoordinates {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexFlags {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtGeometricTransformAttribute {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtPolygonMesh {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtCompressedElementSequence {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtCompressedElement {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtStringPropertyAtom {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtShapeLodBinding {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtBaseNodeData {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtGroupNodeData {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtInstanceNode {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtPartitionNode {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtRangeLodNode {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DisplayJtTriStripShapeNode {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for SegmentIndexRow {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for SegmentStreamLink {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for SegmentBodyBinding {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for SegmentBodyLineageStatus {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockObjectFrame {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for OffsetStoreNamedPoint {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchNamedPointBlockUse {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchPrecedingNamedPointUse {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchDatumCsysDependency {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockAbrReferenceLane {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for SegmentOmLink {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for OmRecordArea {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureOperationLabel {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchRecord {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchPayloadFixedPair {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchFixedPoint {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureOperationRecord {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeaturePayloadString {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureBodyReference {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureBodyReferenceOccurrence {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureInputBlock {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureBooleanOperation {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for ExpressionDeclaration {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlValue {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlForm {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlClassReference {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlIndexValue {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlReference {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlHandlePair {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for DataBlockReference {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for FeatureParameterBinding {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for StoreHeader {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for ExternalReference {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for ExternalReferenceRecord {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for MaterialTextureAsset {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}
impl ContainerNoted for MaterialTextureCatalogEntry {
    fn container_note(&self) -> (&str, u64) {
        (&self.id, self.source_offset)
    }
}

impl StreamNoted for ParasolidBlendSurfaceRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidBlendBoundRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidOffsetSurfaceRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidTrimmedCurveRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidSurfaceCurveRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidTermUseRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidSupportUvRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidChartRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidAttributeDefinition {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity51Record {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity52IntegerRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity53DoubleRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity54StringRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity51StringUse {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity51NumericUse {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidTopologyAttributeListReference {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}

fn note_display_jt_display_jt_indices(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for index in &m.display_jt.display_jt_indices {
        a.note(&index.id, annotation_stream, index.source_offset)
            .tag("DISPLAY_JT_INDEX");
        a.exactness(&index.id, Exactness::ByteExact);
        for row in &index.rows {
            a.note(&row.id, annotation_stream, row.source_offset)
                .tag("DISPLAY_JT_INDEX_ROW");
            a.exactness(&row.id, Exactness::ByteExact);
        }
    }
}

fn note_display_jt_display_jt_documents(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for document in &m.display_jt.display_jt_documents {
        a.note(&document.id, annotation_stream, document.source_offset)
            .tag("DISPLAY_JT_DOCUMENT");
        a.exactness(&document.id, Exactness::ByteExact);
        for entry in &document.toc_entries {
            a.note(&entry.id, annotation_stream, entry.source_offset)
                .tag("DISPLAY_JT_TOC_ENTRY");
            a.exactness(&entry.id, Exactness::ByteExact);
        }
    }
}

fn note_parasolid_parasolid_intersection_records(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    for record in &m.parasolid.parasolid_intersection_records {
        let source_stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, source_stream, record.inflated_offset)
            .tag(if record.delta_twin {
                "INTERSECTION_DATA"
            } else {
                "INTERSECTION"
            });
        a.exactness(&record.id, Exactness::ByteExact);
    }
}

fn note_parasolid_parasolid_attribute_class_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    for class_use in &m.parasolid.parasolid_attribute_class_uses {
        let entity = m
            .parasolid
            .parasolid_entity_51_records
            .iter()
            .find(|entity| entity.id == class_use.entity_51_record)
            .expect("class use owns a type-81 entity");
        let source_stream = a.stream(format!("nx:s{}", class_use.stream_ordinal));
        a.note(&class_use.id, source_stream, entity.inflated_offset)
            .tag("ATTRIBUTE_CLASS_USE");
        a.exactness(&class_use.id, Exactness::Derived);
    }
}

fn note_parasolid_parasolid_topology_attribute_class_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    for class_use in &m.parasolid.parasolid_topology_attribute_class_uses {
        let reference = m
            .parasolid
            .parasolid_topology_attribute_list_references
            .iter()
            .find(|reference| reference.id == class_use.topology_attribute_reference)
            .expect("class use owns a topology attribute reference");
        let source_stream = a.stream(format!("nx:s{}", reference.stream_ordinal));
        let entity = m
            .parasolid
            .parasolid_entity_51_records
            .iter()
            .find(|entity| entity.id == class_use.entity_51_record)
            .expect("class use owns a type-81 entity");
        a.note(&class_use.id, source_stream, entity.inflated_offset)
            .tag("TOPOLOGY_ATTRIBUTE_CLASS_USE");
        a.exactness(&class_use.id, Exactness::Derived);
    }
}

fn note_features_feature_sketch_point_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for point_use in &m.features.feature_sketch_point_uses {
        a.note(
            &point_use.id,
            annotation_stream,
            point_use.source_offsets[0],
        )
        .tag("SKETCH_POINT_USE");
        a.exactness(&point_use.id, Exactness::Derived);
    }
}

fn note_features_feature_input_block_identity_groups(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for group in &m.features.feature_input_block_identity_groups {
        a.note(&group.id, annotation_stream, group.source_offsets[0])
            .tag("FEATURE_INPUT_BLOCK_IDENTITY_GROUP");
        a.exactness(&group.id, Exactness::ByteExact);
    }
}

fn note_features_feature_parameter_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for parameter_use in &m.features.feature_parameter_uses {
        a.note(
            &parameter_use.id,
            annotation_stream,
            parameter_use.source_offsets[0],
        )
        .tag("FEATURE_PARAMETER_USE");
        a.exactness(&parameter_use.id, Exactness::Derived);
    }
}

/// One row per native record family, note-bearing rows in emission order.
pub(crate) const CATALOGUE: &[CatalogueRow] = &[
    CatalogueRow {
        arena: "display_jt_indices",
        tag: None,
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_indices),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_indices, r, ns),
        len: |m| m.display_jt.display_jt_indices.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "display_jt_documents",
        tag: None,
        exactness: Exactness::ByteExact,
        note: Some(note_display_jt_display_jt_documents),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_documents, r, ns),
        len: |m| m.display_jt.display_jt_documents.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_segments",
        tag: Some("DISPLAY_JT_SEGMENT"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_segments, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_segments, r, ns),
        len: |m| m.display_jt.display_jt_segments.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_shape_lod_elements",
        tag: Some("DISPLAY_JT_SHAPE_LOD_ELEMENT"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_shape_lod_elements, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_shape_lod_elements, r, ns),
        len: |m| m.display_jt.display_jt_shape_lod_elements.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_tri_strip_lod_headers",
        tag: Some("DISPLAY_JT_TRI_STRIP_LOD_HEADER"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_tri_strip_lod_headers, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_tri_strip_lod_headers, r, ns),
        len: |m| m.display_jt.display_jt_tri_strip_lod_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_initial_face_degree_symbols",
        tag: Some("DISPLAY_JT_INITIAL_FACE_DEGREE_SYMBOLS"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_container(&m.display_jt.display_jt_initial_face_degree_symbols, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_initial_face_degree_symbols, r, ns),
        len: |m| m.display_jt.display_jt_initial_face_degree_symbols.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_topology_packet_sequences",
        tag: Some("DISPLAY_JT_TOPOLOGY_PACKET_SEQUENCE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_container(&m.display_jt.display_jt_topology_packet_sequences, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_topology_packet_sequences, r, ns),
        len: |m| m.display_jt.display_jt_topology_packet_sequences.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_records_headers",
        tag: Some("DISPLAY_JT_VERTEX_RECORDS_HEADER"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_vertex_records_headers, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_records_headers, r, ns),
        len: |m| m.display_jt.display_jt_vertex_records_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_coordinate_array_headers",
        tag: Some("DISPLAY_JT_COORDINATE_ARRAY_HEADER"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_container(&m.display_jt.display_jt_coordinate_array_headers, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_coordinate_array_headers, r, ns),
        len: |m| m.display_jt.display_jt_coordinate_array_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_coordinates",
        tag: Some("DISPLAY_JT_VERTEX_COORDINATES"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_vertex_coordinates, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_coordinates, r, ns),
        len: |m| m.display_jt.display_jt_vertex_coordinates.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_normals",
        tag: Some("DISPLAY_JT_VERTEX_NORMALS"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_vertex_normals, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_normals, r, ns),
        len: |m| m.display_jt.display_jt_vertex_normals.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_colors",
        tag: Some("DISPLAY_JT_VERTEX_COLORS"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_vertex_colors, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_colors, r, ns),
        len: |m| m.display_jt.display_jt_vertex_colors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_texture_coordinates",
        tag: Some("DISPLAY_JT_VERTEX_TEXTURE_COORDINATES"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| {
            note_container(&m.display_jt.display_jt_vertex_texture_coordinates, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_texture_coordinates, r, ns),
        len: |m| m.display_jt.display_jt_vertex_texture_coordinates.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_flags",
        tag: Some("DISPLAY_JT_VERTEX_FLAGS"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_vertex_flags, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_flags, r, ns),
        len: |m| m.display_jt.display_jt_vertex_flags.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_geometric_transform_attributes",
        tag: Some("DISPLAY_JT_GEOMETRIC_TRANSFORM"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| {
            note_container(
                &m.display_jt.display_jt_geometric_transform_attributes,
                r,
                a,
            );
        }),
        emit: |m, r, ns| {
            emit_arena(
                &m.display_jt.display_jt_geometric_transform_attributes,
                r,
                ns,
            )
        },
        len: |m| m.display_jt.display_jt_geometric_transform_attributes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_polygon_meshes",
        tag: Some("DISPLAY_JT_POLYGON_MESH"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_polygon_meshes, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_polygon_meshes, r, ns),
        len: |m| m.display_jt.display_jt_polygon_meshes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_compressed_element_sequences",
        tag: Some("DISPLAY_JT_COMPRESSED_ELEMENT_SEQUENCE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_container(&m.display_jt.display_jt_compressed_element_sequences, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_compressed_element_sequences, r, ns),
        len: |m| m.display_jt.display_jt_compressed_element_sequences.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_compressed_elements",
        tag: Some("DISPLAY_JT_COMPRESSED_ELEMENT"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_compressed_elements, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_compressed_elements, r, ns),
        len: |m| m.display_jt.display_jt_compressed_elements.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_string_property_atoms",
        tag: Some("DISPLAY_JT_STRING_PROPERTY_ATOM"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_string_property_atoms, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_string_property_atoms, r, ns),
        len: |m| m.display_jt.display_jt_string_property_atoms.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_shape_lod_bindings",
        tag: Some("DISPLAY_JT_SHAPE_LOD_BINDING"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_shape_lod_bindings, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_shape_lod_bindings, r, ns),
        len: |m| m.display_jt.display_jt_shape_lod_bindings.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_base_node_data",
        tag: Some("DISPLAY_JT_BASE_NODE_DATA"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_base_node_data, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_base_node_data, r, ns),
        len: |m| m.display_jt.display_jt_base_node_data.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_group_node_data",
        tag: Some("DISPLAY_JT_GROUP_NODE_DATA"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_group_node_data, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_group_node_data, r, ns),
        len: |m| m.display_jt.display_jt_group_node_data.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_instance_nodes",
        tag: Some("DISPLAY_JT_INSTANCE_NODE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_instance_nodes, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_instance_nodes, r, ns),
        len: |m| m.display_jt.display_jt_instance_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_partition_nodes",
        tag: Some("DISPLAY_JT_PARTITION_NODE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_partition_nodes, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_partition_nodes, r, ns),
        len: |m| m.display_jt.display_jt_partition_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_range_lod_nodes",
        tag: Some("DISPLAY_JT_RANGE_LOD_NODE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_range_lod_nodes, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_range_lod_nodes, r, ns),
        len: |m| m.display_jt.display_jt_range_lod_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_tri_strip_shape_nodes",
        tag: Some("DISPLAY_JT_TRI_STRIP_SHAPE_NODE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.display_jt.display_jt_tri_strip_shape_nodes, r, a)),
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_tri_strip_shape_nodes, r, ns),
        len: |m| m.display_jt.display_jt_tri_strip_shape_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "segment_index_rows",
        tag: Some("UG_PART_SEGMENT_INDEX_ROW"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.segments.segment_index_rows, r, a)),
        emit: |m, r, ns| emit_arena(&m.segments.segment_index_rows, r, ns),
        len: |m| m.segments.segment_index_rows.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_stream_links",
        tag: Some("UG_PART_SEGMENT_STREAM_LINK"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.segments.segment_stream_links, r, a)),
        emit: |m, r, ns| emit_arena(&m.segments.segment_stream_links, r, ns),
        len: |m| m.segments.segment_stream_links.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_body_bindings",
        tag: Some("UG_PART_SEGMENT_BODY_BINDING"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.segments.segment_body_bindings, r, a)),
        emit: |m, r, ns| emit_arena(&m.segments.segment_body_bindings, r, ns),
        len: |m| m.segments.segment_body_bindings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_body_lineage_statuses",
        tag: Some("SEGMENT_BODY_LINEAGE_STATUS"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.segments.segment_body_lineage_statuses, r, a)),
        emit: |m, r, ns| emit_arena(&m.segments.segment_body_lineage_statuses, r, ns),
        len: |m| m.segments.segment_body_lineage_statuses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_blend_surface_records",
        tag: Some("BLEND_SURF"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_blend_surface_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_blend_surface_records, r, ns),
        len: |m| m.parasolid.parasolid_blend_surface_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_blend_bound_records",
        tag: Some("BLEND_BOUND"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_blend_bound_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_blend_bound_records, r, ns),
        len: |m| m.parasolid.parasolid_blend_bound_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_offset_surface_records",
        tag: Some("OFFSET_SURF"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_offset_surface_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_offset_surface_records, r, ns),
        len: |m| m.parasolid.parasolid_offset_surface_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_trimmed_curve_records",
        tag: Some("TRIMMED_CURVE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_trimmed_curve_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_trimmed_curve_records, r, ns),
        len: |m| m.parasolid.parasolid_trimmed_curve_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_surface_curve_records",
        tag: Some("SP_CURVE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_surface_curve_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_surface_curve_records, r, ns),
        len: |m| m.parasolid.parasolid_surface_curve_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_intersection_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: Some(note_parasolid_parasolid_intersection_records),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_intersection_records, r, ns),
        len: |m| m.parasolid.parasolid_intersection_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_term_use_records",
        tag: Some("term_use"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_term_use_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_term_use_records, r, ns),
        len: |m| m.parasolid.parasolid_term_use_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_support_uv_records",
        tag: Some("values"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_support_uv_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_support_uv_records, r, ns),
        len: |m| m.parasolid.parasolid_support_uv_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_chart_records",
        tag: Some("CHART_s"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_chart_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_chart_records, r, ns),
        len: |m| m.parasolid.parasolid_chart_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_attribute_definitions",
        tag: Some("ATTRIBUTE_DEFINITION"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_attribute_definitions, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_attribute_definitions, r, ns),
        len: |m| m.parasolid.parasolid_attribute_definitions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_records",
        tag: Some("ENTITY_51"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_entity_51_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_51_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_51_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_52_integer_records",
        tag: Some("ENTITY_52_INTEGERS"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_per_stream(&m.parasolid.parasolid_entity_52_integer_records, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_52_integer_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_52_integer_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_53_double_records",
        tag: Some("ENTITY_53_DOUBLES"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_per_stream(&m.parasolid.parasolid_entity_53_double_records, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_53_double_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_53_double_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_54_string_records",
        tag: Some("ENTITY_54_STRING"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_per_stream(&m.parasolid.parasolid_entity_54_string_records, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_54_string_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_54_string_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_string_uses",
        tag: Some("ENTITY_51_STRING_USE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_entity_51_string_uses, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_51_string_uses, r, ns),
        len: |m| m.parasolid.parasolid_entity_51_string_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_numeric_uses",
        tag: Some("ENTITY_51_NUMERIC_USE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_per_stream(&m.parasolid.parasolid_entity_51_numeric_uses, r, a)),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_51_numeric_uses, r, ns),
        len: |m| m.parasolid.parasolid_entity_51_numeric_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_attribute_class_uses",
        tag: None,
        exactness: Exactness::Derived,
        note: Some(note_parasolid_parasolid_attribute_class_uses),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_attribute_class_uses, r, ns),
        len: |m| m.parasolid.parasolid_attribute_class_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_topology_attribute_list_references",
        tag: Some("TOPOLOGY_ATTRIBUTE_LIST_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_per_stream(
                &m.parasolid.parasolid_topology_attribute_list_references,
                r,
                a,
            );
        }),
        emit: |m, r, ns| {
            emit_arena(
                &m.parasolid.parasolid_topology_attribute_list_references,
                r,
                ns,
            )
        },
        len: |m| {
            m.parasolid
                .parasolid_topology_attribute_list_references
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_topology_attribute_class_uses",
        tag: None,
        exactness: Exactness::Derived,
        note: Some(note_parasolid_parasolid_topology_attribute_class_uses),
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_topology_attribute_class_uses, r, ns),
        len: |m| m.parasolid.parasolid_topology_attribute_class_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_object_frames",
        tag: Some("OFFSET_STORE_OBJECT_FRAME"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.data_block_object_frames, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.data_block_object_frames, r, ns),
        len: |m| m.features.data_block_object_frames.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "offset_store_named_points",
        tag: Some("OFFSET_STORE_NAMED_POINT"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.offset_store_named_points, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.offset_store_named_points, r, ns),
        len: |m| m.features.offset_store_named_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_named_point_block_uses",
        tag: Some("SKETCH_NAMED_POINT_BLOCK_USE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_container(&m.features.feature_sketch_named_point_block_uses, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_named_point_block_uses, r, ns),
        len: |m| m.features.feature_sketch_named_point_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_preceding_named_point_uses",
        tag: Some("SKETCH_PRECEDING_NAMED_POINT_USE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| {
            note_container(&m.features.feature_sketch_preceding_named_point_uses, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_preceding_named_point_uses, r, ns),
        len: |m| m.features.feature_sketch_preceding_named_point_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_point_uses",
        tag: None,
        exactness: Exactness::Derived,
        note: Some(note_features_feature_sketch_point_uses),
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_point_uses, r, ns),
        len: |m| m.features.feature_sketch_point_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_datum_csys_dependencies",
        tag: Some("SKETCH_DATUM_CSYS_DEPENDENCY"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| {
            note_container(&m.features.feature_sketch_datum_csys_dependencies, r, a);
        }),
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_datum_csys_dependencies, r, ns),
        len: |m| m.features.feature_sketch_datum_csys_dependencies.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_input_block_identity_groups",
        tag: None,
        exactness: Exactness::ByteExact,
        note: Some(note_features_feature_input_block_identity_groups),
        emit: |m, r, ns| emit_arena(&m.features.feature_input_block_identity_groups, r, ns),
        len: |m| m.features.feature_input_block_identity_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_abr_reference_lanes",
        tag: Some("OFFSET_STORE_ABR_REFERENCE_LANE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.data_block_abr_reference_lanes, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.data_block_abr_reference_lanes, r, ns),
        len: |m| m.om.data_block_abr_reference_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_om_links",
        tag: Some("UG_PART_SEGMENT_OM_LINK"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.segments.segment_om_links, r, a)),
        emit: |m, r, ns| emit_arena(&m.segments.segment_om_links, r, ns),
        len: |m| m.segments.segment_om_links.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_record_areas",
        tag: Some("OM_RECORD_AREA"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.om_record_areas, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.om_record_areas, r, ns),
        len: |m| m.om.om_record_areas.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_labels",
        tag: Some("FEATURE_OPERATION_LABEL"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.feature_operation_labels, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_labels, r, ns),
        len: |m| m.features.feature_operation_labels.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_records",
        tag: Some("FEATURE_SKETCH_RECORD"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.features.feature_sketch_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_records, r, ns),
        len: |m| m.features.feature_sketch_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_fixed_pairs",
        tag: Some("FEATURE_SKETCH_FIXED_PAIR"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.feature_sketch_payload_fixed_pairs, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_fixed_pairs, r, ns),
        len: |m| m.features.feature_sketch_payload_fixed_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_fixed_points",
        tag: Some("FEATURE_SKETCH_FIXED_POINT"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.features.feature_sketch_fixed_points, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_fixed_points, r, ns),
        len: |m| m.features.feature_sketch_fixed_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_records",
        tag: Some("FEATURE_OPERATION_RECORD"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.feature_operation_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_records, r, ns),
        len: |m| m.features.feature_operation_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_payload_strings",
        tag: Some("FEATURE_PAYLOAD_STRING"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.feature_payload_strings, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_payload_strings, r, ns),
        len: |m| m.features.feature_payload_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_references",
        tag: Some("FEATURE_BODY_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.feature_body_references, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_body_references, r, ns),
        len: |m| m.features.feature_body_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_reference_occurrences",
        tag: Some("FEATURE_BODY_REFERENCE_OCCURRENCE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.feature_body_reference_occurrences, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_body_reference_occurrences, r, ns),
        len: |m| m.features.feature_body_reference_occurrences.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_blocks",
        tag: Some("FEATURE_INPUT_BLOCK"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.feature_input_blocks, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_input_blocks, r, ns),
        len: |m| m.features.feature_input_blocks.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_boolean_operations",
        tag: Some("FEATURE_BOOLEAN_OPERATION"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.features.feature_boolean_operations, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_boolean_operations, r, ns),
        len: |m| m.features.feature_boolean_operations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "expression_declarations",
        tag: Some("EXPRESSION_DECLARATION"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.expression_declarations, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.expression_declarations, r, ns),
        len: |m| m.om.expression_declarations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_forms",
        tag: Some("OM_DATA_BLOCK_CONTROL_FORM"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.data_block_control_forms, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_forms, r, ns),
        len: |m| m.om.data_block_control_forms.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_values",
        tag: Some("OM_DATA_BLOCK_CONTROL_VALUE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.data_block_control_values, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_values, r, ns),
        len: |m| m.om.data_block_control_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_class_references",
        tag: Some("OM_DATA_BLOCK_CONTROL_CLASS_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.data_block_control_class_references, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_class_references, r, ns),
        len: |m| m.om.data_block_control_class_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_index_values",
        tag: Some("OM_DATA_BLOCK_CONTROL_INDEX_VALUE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.data_block_control_index_values, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_index_values, r, ns),
        len: |m| m.om.data_block_control_index_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_references",
        tag: Some("OM_DATA_BLOCK_CONTROL_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.data_block_control_references, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_references, r, ns),
        len: |m| m.om.data_block_control_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_handle_pairs",
        tag: Some("OM_DATA_BLOCK_CONTROL_HANDLE_PAIR"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.data_block_control_handle_pairs, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_handle_pairs, r, ns),
        len: |m| m.om.data_block_control_handle_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_references",
        tag: Some("OM_DATA_BLOCK_REFERENCE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.data_block_references, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.data_block_references, r, ns),
        len: |m| m.om.data_block_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_parameter_bindings",
        tag: Some("FEATURE_PARAMETER_BINDING"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.features.feature_parameter_bindings, r, a)),
        emit: |m, r, ns| emit_arena(&m.features.feature_parameter_bindings, r, ns),
        len: |m| m.features.feature_parameter_bindings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_parameter_uses",
        tag: None,
        exactness: Exactness::Derived,
        note: Some(note_features_feature_parameter_uses),
        emit: |m, r, ns| emit_arena(&m.features.feature_parameter_uses, r, ns),
        len: |m| m.features.feature_parameter_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "store_headers",
        tag: Some("OM_STORE_VERSION"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.store_headers, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.store_headers, r, ns),
        len: |m| m.om.store_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_references",
        tag: Some("EXTREFSTREAM_STRING"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.external_references, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.external_references, r, ns),
        len: |m| m.om.external_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_records",
        tag: Some("EXTREFSTREAM_RECORD"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.external_reference_records, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.external_reference_records, r, ns),
        len: |m| m.om.external_reference_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "material_texture_assets",
        tag: Some("TIFF_MATERIAL_TEXTURE"),
        exactness: Exactness::ByteExact,
        note: Some(|m, r, a| note_container(&m.om.material_texture_assets, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.material_texture_assets, r, ns),
        len: |m| m.om.material_texture_assets.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "material_texture_catalog_entries",
        tag: Some("QAF_MATERIAL_TEXTURE_CATALOG_ENTRY"),
        exactness: Exactness::Derived,
        note: Some(|m, r, a| note_container(&m.om.material_texture_catalog_entries, r, a)),
        emit: |m, r, ns| emit_arena(&m.om.material_texture_catalog_entries, r, ns),
        len: |m| m.om.material_texture_catalog_entries.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_segment_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_body_segment_uses, r, ns),
        len: |m| m.features.feature_body_segment_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_simple_hole_templates",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_simple_hole_templates, r, ns),
        len: |m| m.features.feature_simple_hole_templates.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_repeated_scalar_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_simple_hole_repeated_scalar_lanes, r, ns),
        len: |m| m.features.feature_simple_hole_repeated_scalar_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_repeated_scalar_lane_block_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| {
            emit_arena(
                &m.features
                    .feature_simple_hole_repeated_scalar_lane_block_references,
                r,
                ns,
            )
        },
        len: |m| {
            m.features
                .feature_simple_hole_repeated_scalar_lane_block_references
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_construction_groups",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_simple_hole_construction_groups, r, ns),
        len: |m| m.features.feature_simple_hole_construction_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_csys_constructions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_constructions, r, ns),
        len: |m| m.features.feature_datum_csys_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_payloads, r, ns),
        len: |m| m.features.feature_datum_csys_payloads.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_scalar_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_payload_scalar_pairs, r, ns),
        len: |m| m.features.feature_datum_csys_payload_scalar_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_fixed_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_payload_fixed_pairs, r, ns),
        len: |m| m.features.feature_datum_csys_payload_fixed_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_scalars",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_payload_scalars, r, ns),
        len: |m| m.features.feature_datum_csys_payload_scalars.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_descriptors",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_descriptors, r, ns),
        len: |m| m.features.feature_datum_csys_descriptors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_block_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_block_uses, r, ns),
        len: |m| m.features.feature_datum_csys_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_headers",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_headers, r, ns),
        len: |m| m.features.feature_datum_plane_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_block_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_block_uses, r, ns),
        len: |m| m.features.feature_datum_plane_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_payloads, r, ns),
        len: |m| m.features.feature_datum_plane_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_payload_scalar_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_payload_scalar_pairs, r, ns),
        len: |m| m.features.feature_datum_plane_payload_scalar_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_plane_descriptors",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_descriptors, r, ns),
        len: |m| m.features.feature_datum_plane_descriptors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_plane_csys_identity_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_csys_identity_uses, r, ns),
        len: |m| m.features.feature_datum_plane_csys_identity_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_references, r, ns),
        len: |m| m.features.feature_sketch_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_projected_curve_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_projected_curve_references, r, ns),
        len: |m| m.features.feature_projected_curve_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_projected_curve_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| {
            emit_arena(
                &m.features.feature_projected_curve_construction_payloads,
                r,
                ns,
            )
        },
        len: |m| {
            m.features
                .feature_projected_curve_construction_payloads
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_projected_curve_construction_strings",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| {
            emit_arena(
                &m.features.feature_projected_curve_construction_strings,
                r,
                ns,
            )
        },
        len: |m| {
            m.features
                .feature_projected_curve_construction_strings
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_references, r, ns),
        len: |m| m.features.feature_pattern_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_construction_payloads, r, ns),
        len: |m| m.features.feature_pattern_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_strings",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_construction_strings, r, ns),
        len: |m| m.features.feature_pattern_construction_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_fixed_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_construction_fixed_lanes, r, ns),
        len: |m| m.features.feature_pattern_construction_fixed_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_transform_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_transform_lanes, r, ns),
        len: |m| m.features.feature_pattern_transform_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_point_construction_headers",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_point_construction_headers, r, ns),
        len: |m| m.features.feature_point_construction_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_point_construction_scalar_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_point_construction_scalar_lanes, r, ns),
        len: |m| m.features.feature_point_construction_scalar_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_references, r, ns),
        len: |m| m.features.feature_draft_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_index_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_index_lanes, r, ns),
        len: |m| m.features.feature_draft_construction_index_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_payloads, r, ns),
        len: |m| m.features.feature_draft_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_graph_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_graph_payloads, r, ns),
        len: |m| m.features.feature_draft_construction_graph_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_fixed_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_fixed_lanes, r, ns),
        len: |m| m.features.feature_draft_construction_fixed_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_binary32_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_binary32_lanes, r, ns),
        len: |m| m.features.feature_draft_construction_binary32_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_graph_strings",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_graph_strings, r, ns),
        len: |m| m.features.feature_draft_construction_graph_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_identity_frames",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| {
            emit_arena(
                &m.features.feature_draft_construction_identity_frames,
                r,
                ns,
            )
        },
        len: |m| m.features.feature_draft_construction_identity_frames.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_terminal_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_terminal_lanes, r, ns),
        len: |m| m.features.feature_draft_construction_terminal_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_references, r, ns),
        len: |m| m.features.feature_surface_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_payloads, r, ns),
        len: |m| m.features.feature_surface_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_scalar_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_scalar_pairs, r, ns),
        len: |m| m.features.feature_surface_construction_scalar_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_strings",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_strings, r, ns),
        len: |m| m.features.feature_surface_construction_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_branches",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_branches, r, ns),
        len: |m| m.features.feature_surface_construction_branches.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_profile_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_profile_references, r, ns),
        len: |m| m.features.feature_extrude_profile_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_payload_headers",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_payload_headers, r, ns),
        len: |m| m.features.feature_extrude_payload_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_payload_footers",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_payload_footers, r, ns),
        len: |m| m.features.feature_extrude_payload_footers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_scalar_triples",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_scalar_triples, r, ns),
        len: |m| m.features.feature_operation_body_scalar_triples.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_members",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_members, r, ns),
        len: |m| m.features.feature_operation_body_members.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_operands",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_operands, r, ns),
        len: |m| m.features.feature_operation_body_operands.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_11_continuations",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_11_continuations, r, ns),
        len: |m| m.features.feature_operation_body_11_continuations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_reference_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_reference_lanes, r, ns),
        len: |m| m.features.feature_operation_body_reference_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_construction_profiles",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_construction_profiles, r, ns),
        len: |m| m.features.feature_extrude_construction_profiles.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_payload_32_branches",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_payload_32_branches, r, ns),
        len: |m| m.features.feature_extrude_payload_32_branches.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_32_constructions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_32_constructions, r, ns),
        len: |m| m.features.feature_extrude_32_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_construction_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_construction_references, r, ns),
        len: |m| m.features.feature_block_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_constructions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_constructions, r, ns),
        len: |m| m.features.feature_block_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_construction_payloads, r, ns),
        len: |m| m.features.feature_block_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_scalars",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_scalars, r, ns),
        len: |m| m.features.feature_block_payload_scalars.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_names",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_names, r, ns),
        len: |m| m.features.feature_block_payload_names.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_named_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_named_records, r, ns),
        len: |m| m.features.feature_block_payload_named_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_points",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_points, r, ns),
        len: |m| m.features.feature_block_payload_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_point_groups",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_point_groups, r, ns),
        len: |m| m.features.feature_block_payload_point_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_dimensions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_dimensions, r, ns),
        len: |m| m.features.feature_block_dimensions.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_construction_inputs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_construction_inputs, r, ns),
        len: |m| m.features.feature_sketch_construction_inputs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_construction_payloads",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_construction_payloads, r, ns),
        len: |m| m.features.feature_sketch_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_coordinate_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_coordinate_pairs, r, ns),
        len: |m| m.features.feature_sketch_payload_coordinate_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_scalars",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_scalars, r, ns),
        len: |m| m.features.feature_sketch_payload_scalars.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_names",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_names, r, ns),
        len: |m| m.features.feature_sketch_payload_names.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_named_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_named_records, r, ns),
        len: |m| m.features.feature_sketch_payload_named_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_points",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_points, r, ns),
        len: |m| m.features.feature_sketch_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_point_groups",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_point_groups, r, ns),
        len: |m| m.features.feature_sketch_point_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "expressions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.expressions, r, ns),
        len: |m| m.om.expressions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "class_definitions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.classes, r, ns),
        len: |m| m.om.classes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "field_definitions",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.fields, r, ns),
        len: |m| m.om.fields.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "object_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.object_records, r, ns),
        len: |m| m.om.object_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "rmfastload_object_id_tables",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.rmfastload_object_id_tables, r, ns),
        len: |m| m.om.rmfastload_object_id_tables.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "rmfastload_object_ids",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.rmfastload_object_ids, r, ns),
        len: |m| m.om.rmfastload_object_ids.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_blocks",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.data_blocks, r, ns),
        len: |m| m.om.data_blocks.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_counted_index_lanes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.data_block_counted_index_lanes, r, ns),
        len: |m| m.om.data_block_counted_index_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_index_rows",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.data_block_index_rows, r, ns),
        len: |m| m.om.data_block_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_linked_index_rows",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.data_block_linked_index_rows, r, ns),
        len: |m| m.om.data_block_linked_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_target_index_rows",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.data_block_target_index_rows, r, ns),
        len: |m| m.om.data_block_target_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_column_index_tables",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.data_block_column_index_tables, r, ns),
        len: |m| m.om.data_block_column_index_tables.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_column_row_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_input_column_row_uses, r, ns),
        len: |m| m.features.feature_input_column_row_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_column_targets",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.features.feature_input_column_targets, r, ns),
        len: |m| m.features.feature_input_column_targets.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "string_values",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.string_values, r, ns),
        len: |m| m.om.string_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "object_references",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.object_references, r, ns),
        len: |m| m.om.object_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "persistent_handles",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.persistent_handles, r, ns),
        len: |m| m.om.persistent_handles.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "configurations",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.configurations, r, ns),
        len: |m| m.om.configurations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "configuration_attribute_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.configuration_attribute_uses, r, ns),
        len: |m| m.om.configuration_attribute_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "part_attributes",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.part_attributes, r, ns),
        len: |m| m.om.part_attributes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_indexed_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_indexed_records, r, ns),
        len: |m| m.om.external_reference_indexed_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_empty_records",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_empty_records, r, ns),
        len: |m| m.om.external_reference_empty_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_tail_reference_pairs",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_tail_reference_pairs, r, ns),
        len: |m| m.om.external_reference_tail_reference_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_record_string_uses",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_record_string_uses, r, ns),
        len: |m| m.om.external_reference_record_string_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_record_children",
        tag: None,
        exactness: Exactness::ByteExact,
        note: None,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_record_children, r, ns),
        len: |m| m.om.external_reference_record_children.len(),
        counts_toward_emptiness: true,
    },
];
