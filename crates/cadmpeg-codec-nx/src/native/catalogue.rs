// SPDX-License-Identifier: Apache-2.0
//! Declarative catalogue of the native record families.
//!
//! One [`FamilyRow`] per model field: arena name, and for noting families the
//! tag, exactness, and `note` fn. Note-bearing row order is the annotation
//! emission order; `phase` splits semantic islands for [`super::attach`].
//! Stream choice (`nx:container` vs `nx:s{ordinal}`) lives in the `note` fn.

use super::features::operation_record::FeatureOperationRecord;
use super::features::unlabeled_record::FeatureUnlabeledOperationRecord;
use crate::native::features::object_frame::DataBlockObjectFrame;
use crate::native::om::compact_lane::DataBlockAbrReferenceLane;
use crate::native::om::journal_group::OmOperationStateJournalGroup;
use crate::native::om::material_texture::MaterialTextureAsset;
use crate::native::om::object_uuid::ObjectUuidValue;
use crate::native::om::roll_forward::{OmRollForwardStateGroup, OmRollForwardStateTable};
use crate::native::om::state_slot_lane::OmOperationStateSlotLane;
use crate::native::om::state_status::OmOperationStateStatus;

use std::borrow::Cow;

use serde::Serialize;

use cadmpeg_ir::native::catalogue::{Catalogue, FamilyRow, Phase};
use cadmpeg_ir::{AnnotationBuilder, Exactness, NativeConvertError, NativeNamespace};

use super::model::NativeModel;
#[allow(clippy::wildcard_imports)]
use super::{
    display_jt::*, features::*, om::*, parasolid::*, segments::*, structure::*, toggle::*,
};

pub(crate) type CatalogueRow =
    FamilyRow<NativeModel, AnnotationBuilder, NativeNamespace, Exactness>;

/// Serialize a record family into its arena when non-empty.
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
    fn container_note(&self) -> (Cow<'_, str>, u64);
}

impl ContainerNoted for SavedToggleStream {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(SavedToggleStream::id()), self.source_offset)
    }
}

impl ContainerNoted for SavedToggleEntry {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Owned(self.id()), self.source_offset())
    }
}

/// A record noted into its own `nx:s{ordinal}` stream: its id, the stream
/// ordinal, and the inflated-byte offset the note points at.
trait StreamNoted {
    fn stream_note(&self) -> (&str, u32, u64);
}

/// Emit the standard container-stream note for every record in a family: one
/// `nx:container` note at the record's source offset with the phase's optional tag,
/// plus the row's exactness.
fn note_container<T: ContainerNoted>(
    records: &[T],
    catalogue_row: &CatalogueRow,
    tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    let stream = a.stream("nx:container");
    for record in records {
        let (id, offset) = record.container_note();
        let note = a.note(&id, &stream, offset);
        if let Some(tag) = tag {
            note.tag(tag);
        }
        a.exactness(id, catalogue_row.exactness);
    }
}

/// Emit the standard per-stream note for every record in a family: one note in
/// the record's own `nx:s{ordinal}` stream at its inflated offset tagged with
/// the phase's optional tag, plus the row's exactness.
fn note_per_stream<T: StreamNoted>(
    records: &[T],
    catalogue_row: &CatalogueRow,
    tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    for record in records {
        let (id, stream_ordinal, offset) = record.stream_note();
        let stream = a.stream(format!("nx:s{stream_ordinal}"));
        let note = a.note(id, &stream, offset);
        if let Some(tag) = tag {
            note.tag(tag);
        }
        a.exactness(id, catalogue_row.exactness);
    }
}

impl ContainerNoted for DisplayJtSegment {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FastLoadComponentPrototype {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FastLoadComponentOccurrence {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FastLoadComponentUuid {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtShapeLodElement {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtTriStripLodHeader {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtInitialFaceDegreeSymbols {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtTopologyPacketSequence {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtCompressedVertexRecordsHeader {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexCoordinateArrayHeader {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexCoordinates {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexNormals {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexColors {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexTextureCoordinates {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtVertexFlags {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtGeometricTransformAttribute {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtMaterialAttribute {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtPolygonMesh {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtCompressedElementSequence {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtCompressedElement {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtStringPropertyAtom {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtShapeLodBinding {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtBaseNodeData {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtGroupNodeData {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtInstanceNode {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtPartitionNode {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtRangeLodNode {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DisplayJtTriStripShapeNode {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for SegmentIndexRow {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for SegmentStreamLink {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for SegmentBodyBinding {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for SegmentBodyLineageStatus {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockObjectFrame {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.object.offset)
    }
}
impl ContainerNoted for OffsetStoreNamedPoint {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchNamedPointBlockUse {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchPrecedingNamedPointUse {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchDatumCsysDependency {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockAbrReferenceLane {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for SegmentOmLink {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.location.source_offset())
    }
}
impl ContainerNoted for OmRecordArea {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for OmAuditTrailRow {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset())
    }
}
impl ContainerNoted for OmOperationStateJournalGroup {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for OmOperationStateCounter {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for OmRollForwardStateGroup {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for OmOperationStateMessage {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for OmOperationStateStatus {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset())
    }
}
impl ContainerNoted for OmOperationStateSlotLane {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for FeatureOperationLabel {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchRecord {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchPayloadFixedPair {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchPayloadMixedPair {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureSketchFixedPoint {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureOperationRecord {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.span.source_offset())
    }
}
impl ContainerNoted for FeatureUnlabeledOperationRecord {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset())
    }
}
impl ContainerNoted for FeatureOperationBodyWrite {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for FeatureOperationObjectReference {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for FeatureOperationCommonFrame {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for FeatureOperationTerminalFrame {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.frame.offset())
    }
}
impl ContainerNoted for FeatureOperationStateJournalUse {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.operation_source_offset)
    }
}
impl ContainerNoted for FeaturePayloadString {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureBodyReference {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureInputBlock {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureBooleanOperation {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for ExpressionDeclaration {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlValue {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlForm {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlClassReference {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlIndexValue {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlReference {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockControlHandlePair {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for ObjectRecordHandlePair {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for ObjectUuidValue {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FastLoadComponentObjectGroup {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for DataBlockReference {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for FeatureParameterBinding {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for StoreHeader {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (
            Cow::Borrowed(&self.header().id),
            self.header().source_offset,
        )
    }
}
impl ContainerNoted for ExternalReference {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for ExternalReferenceRecord {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for MaterialTextureAsset {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
    }
}
impl ContainerNoted for MaterialTextureCatalogEntry {
    fn container_note(&self) -> (Cow<'_, str>, u64) {
        (Cow::Borrowed(&self.id), self.source_offset)
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
impl StreamNoted for ParasolidDeltasTransmitHeader {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, 0)
    }
}
impl StreamNoted for ParasolidDeltasTerminalNullReferences {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidGroupRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.origin.stream_ordinal(), self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasTombstone {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasBodyRevision {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasTermUseNumericTail {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasTaggedReferenceLane {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasReferenceTypeMap {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasReferenceStatePacket {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasSchemaReferencePreamble {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasReferenceMarkerPacket {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasType150StatePacket {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasInlineSchemaDeclaration {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasInlineBodyState {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidDeltasResidualSpan {
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
impl StreamNoted for ParasolidEntityVectorRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity57AxisRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity58TagRecord {
    fn stream_note(&self) -> (&str, u32, u64) {
        (&self.id, self.stream_ordinal, self.inflated_offset)
    }
}
impl StreamNoted for ParasolidEntity62UnicodeRecord {
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
impl StreamNoted for ParasolidEntity51StructuredUse {
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
    _tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for index in &m.display_jt.display_jt_indices {
        a.note(&index.id, &annotation_stream, index.source_offset)
            .tag("DISPLAY_JT_INDEX");
        a.exactness(&index.id, Exactness::ByteExact);
        for row in index.rows() {
            a.note(&row.id, &annotation_stream, row.source_offset)
                .tag("DISPLAY_JT_INDEX_ROW");
            a.exactness(&row.id, Exactness::ByteExact);
        }
    }
}

fn note_display_jt_display_jt_documents(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    _tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for document in m.display_jt.graph.documents() {
        a.note(&document.id, &annotation_stream, document.source_offset)
            .tag("DISPLAY_JT_DOCUMENT");
        a.exactness(&document.id, Exactness::ByteExact);
        for entry in &document.toc_entries {
            a.note(&entry.id, &annotation_stream, entry.source_offset)
                .tag("DISPLAY_JT_TOC_ENTRY");
            a.exactness(&entry.id, Exactness::ByteExact);
        }
    }
}

fn note_parasolid_parasolid_intersection_records(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    _tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    for record in &m.parasolid.parasolid_intersection_records {
        let source_stream = a.stream(format!("nx:s{}", record.stream_ordinal));
        a.note(&record.id, &source_stream, record.inflated_offset)
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
    _tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    for class_use in &m.parasolid.parasolid_attribute_class_uses {
        let source_stream = a.stream(format!("nx:s{}", class_use.stream_ordinal));
        a.note(&class_use.id, &source_stream, class_use.inflated_offset)
            .tag("ATTRIBUTE_CLASS_USE");
        a.exactness(&class_use.id, Exactness::Derived);
    }
}

fn note_parasolid_parasolid_topology_attribute_class_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    _tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    for class_use in &m.parasolid.parasolid_topology_attribute_class_uses {
        let source_stream = a.stream(format!("nx:s{}", class_use.stream_ordinal));
        a.note(&class_use.id, &source_stream, class_use.inflated_offset)
            .tag("TOPOLOGY_ATTRIBUTE_CLASS_USE");
        a.exactness(&class_use.id, Exactness::Derived);
    }
}

fn note_features_feature_sketch_point_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    _tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for point_use in &m.features.feature_sketch_point_uses {
        a.note(
            &point_use.id,
            &annotation_stream,
            point_use.references[0].source_offset,
        )
        .tag("SKETCH_POINT_USE");
        a.exactness(&point_use.id, Exactness::Derived);
    }
}

fn note_features_feature_input_block_identity_groups(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    _tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for group in &m.features.feature_input_block_identity_groups {
        a.note(
            &group.id,
            &annotation_stream,
            group.members[0].source_offset,
        )
        .tag("FEATURE_INPUT_BLOCK_IDENTITY_GROUP");
        a.exactness(&group.id, Exactness::ByteExact);
    }
}

fn note_features_feature_parameter_uses(
    m: &NativeModel,
    _catalogue_row: &CatalogueRow,
    _tag: Option<&'static str>,
    a: &mut AnnotationBuilder,
) {
    let annotation_stream = a.stream("nx:container");
    for parameter_use in &m.features.feature_parameter_uses {
        a.note(
            &parameter_use.id,
            &annotation_stream,
            parameter_use.bindings[0].source_offset,
        )
        .tag("FEATURE_PARAMETER_USE");
        a.exactness(&parameter_use.id, Exactness::Derived);
    }
}

/// One row per native record family, note-bearing rows in emission order.
pub(crate) const CATALOGUE: &[CatalogueRow] = &[
    CatalogueRow {
        arena: "display_jt_indices",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: None,
            note: note_display_jt_display_jt_indices,
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_indices, r, ns),
        len: |m| m.display_jt.display_jt_indices.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "display_jt_documents",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: None,
            note: note_display_jt_display_jt_documents,
        },
        emit: |m, r, ns| emit_arena(m.display_jt.graph.documents(), r, ns),
        len: |m| m.display_jt.graph.documents().len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_segments",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_SEGMENT"),
            note: |m, r, tag, a| note_container(m.display_jt.graph.segments(), r, tag, a),
        },
        emit: |m, r, ns| emit_arena(m.display_jt.graph.segments(), r, ns),
        len: |m| m.display_jt.graph.segments().len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_shape_lod_elements",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_SHAPE_LOD_ELEMENT"),
            note: |m, r, tag, a| {
                note_container(m.display_jt.graph.shape_lod_elements(), r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(m.display_jt.graph.shape_lod_elements(), r, ns),
        len: |m| m.display_jt.graph.shape_lod_elements().len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_tri_strip_lod_headers",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_TRI_STRIP_LOD_HEADER"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_tri_strip_lod_headers, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_tri_strip_lod_headers, r, ns),
        len: |m| m.display_jt.display_jt_tri_strip_lod_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_initial_face_degree_symbols",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_INITIAL_FACE_DEGREE_SYMBOLS"),
            note: |m, r, tag, a| {
                note_container(
                    &m.display_jt.display_jt_initial_face_degree_symbols,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_initial_face_degree_symbols, r, ns),
        len: |m| m.display_jt.display_jt_initial_face_degree_symbols.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_topology_packet_sequences",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_TOPOLOGY_PACKET_SEQUENCE"),
            note: |m, r, tag, a| {
                note_container(
                    &m.display_jt.display_jt_topology_packet_sequences,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_topology_packet_sequences, r, ns),
        len: |m| m.display_jt.display_jt_topology_packet_sequences.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_records_headers",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_VERTEX_RECORDS_HEADER"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_vertex_records_headers, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_records_headers, r, ns),
        len: |m| m.display_jt.display_jt_vertex_records_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_coordinate_array_headers",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_COORDINATE_ARRAY_HEADER"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_coordinate_array_headers, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_coordinate_array_headers, r, ns),
        len: |m| m.display_jt.display_jt_coordinate_array_headers.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_coordinates",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_VERTEX_COORDINATES"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_vertex_coordinates, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_coordinates, r, ns),
        len: |m| m.display_jt.display_jt_vertex_coordinates.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_normals",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_VERTEX_NORMALS"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_vertex_normals, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_normals, r, ns),
        len: |m| m.display_jt.display_jt_vertex_normals.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_colors",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_VERTEX_COLORS"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_vertex_colors, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_colors, r, ns),
        len: |m| m.display_jt.display_jt_vertex_colors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_texture_coordinates",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_VERTEX_TEXTURE_COORDINATES"),
            note: |m, r, tag, a| {
                note_container(
                    &m.display_jt.display_jt_vertex_texture_coordinates,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_texture_coordinates, r, ns),
        len: |m| m.display_jt.display_jt_vertex_texture_coordinates.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_vertex_flags",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_VERTEX_FLAGS"),
            note: |m, r, tag, a| note_container(&m.display_jt.display_jt_vertex_flags, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_vertex_flags, r, ns),
        len: |m| m.display_jt.display_jt_vertex_flags.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_geometric_transform_attributes",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_GEOMETRIC_TRANSFORM"),
            note: |m, r, tag, a| {
                note_container(
                    &m.display_jt.display_jt_geometric_transform_attributes,
                    r,
                    tag,
                    a,
                );
            },
        },
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
        arena: "display_jt_material_attributes",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_MATERIAL_ATTRIBUTE"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_material_attributes, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_material_attributes, r, ns),
        len: |m| m.display_jt.display_jt_material_attributes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_polygon_meshes",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_POLYGON_MESH"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_polygon_meshes, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_polygon_meshes, r, ns),
        len: |m| m.display_jt.display_jt_polygon_meshes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_compressed_element_sequences",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_COMPRESSED_ELEMENT_SEQUENCE"),
            note: |m, r, tag, a| {
                note_container(m.display_jt.graph.compressed_element_sequences(), r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(m.display_jt.graph.compressed_element_sequences(), r, ns),
        len: |m| m.display_jt.graph.compressed_element_sequences().len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_compressed_elements",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_COMPRESSED_ELEMENT"),
            note: |m, r, tag, a| {
                note_container(m.display_jt.graph.compressed_elements(), r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(m.display_jt.graph.compressed_elements(), r, ns),
        len: |m| m.display_jt.graph.compressed_elements().len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_string_property_atoms",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_STRING_PROPERTY_ATOM"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_string_property_atoms, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_string_property_atoms, r, ns),
        len: |m| m.display_jt.display_jt_string_property_atoms.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_shape_lod_bindings",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_SHAPE_LOD_BINDING"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_shape_lod_bindings, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_shape_lod_bindings, r, ns),
        len: |m| m.display_jt.display_jt_shape_lod_bindings.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_base_node_data",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_BASE_NODE_DATA"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_base_node_data, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_base_node_data, r, ns),
        len: |m| m.display_jt.display_jt_base_node_data.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_group_node_data",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_GROUP_NODE_DATA"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_group_node_data, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_group_node_data, r, ns),
        len: |m| m.display_jt.display_jt_group_node_data.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_instance_nodes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_INSTANCE_NODE"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_instance_nodes, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_instance_nodes, r, ns),
        len: |m| m.display_jt.display_jt_instance_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_partition_nodes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_PARTITION_NODE"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_partition_nodes, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_partition_nodes, r, ns),
        len: |m| m.display_jt.display_jt_partition_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_range_lod_nodes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_RANGE_LOD_NODE"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_range_lod_nodes, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_range_lod_nodes, r, ns),
        len: |m| m.display_jt.display_jt_range_lod_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "display_jt_tri_strip_shape_nodes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DISPLAY_JT_TRI_STRIP_SHAPE_NODE"),
            note: |m, r, tag, a| {
                note_container(&m.display_jt.display_jt_tri_strip_shape_nodes, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.display_jt.display_jt_tri_strip_shape_nodes, r, ns),
        len: |m| m.display_jt.display_jt_tri_strip_shape_nodes.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "segment_index_rows",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("UG_PART_SEGMENT_INDEX_ROW"),
            note: |m, r, tag, a| note_container(&m.segments.segment_index_rows, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.segments.segment_index_rows, r, ns),
        len: |m| m.segments.segment_index_rows.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_stream_links",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("UG_PART_SEGMENT_STREAM_LINK"),
            note: |m, r, tag, a| note_container(&m.segments.segment_stream_links, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.segments.segment_stream_links, r, ns),
        len: |m| m.segments.segment_stream_links.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_body_bindings",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("UG_PART_SEGMENT_BODY_BINDING"),
            note: |m, r, tag, a| note_container(&m.segments.segment_body_bindings, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.segments.segment_body_bindings, r, ns),
        len: |m| m.segments.segment_body_bindings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_body_lineage_statuses",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("SEGMENT_BODY_LINEAGE_STATUS"),
            note: |m, r, tag, a| {
                note_container(&m.segments.segment_body_lineage_statuses, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.segments.segment_body_lineage_statuses, r, ns),
        len: |m| m.segments.segment_body_lineage_statuses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_group_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("PARASOLID_GROUP_RECORD"),
            note: |m, r, tag, a| note_per_stream(&m.parasolid.parasolid_group_records, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_group_records, r, ns),
        len: |m| m.parasolid.parasolid_group_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_group_members",
        exactness: Exactness::Derived,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_group_members, r, ns),
        len: |m| m.parasolid.parasolid_group_members.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_transmit_headers",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_TRANSMIT_HEADER"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_deltas_transmit_headers, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_transmit_headers, r, ns),
        len: |m| m.parasolid.parasolid_deltas_transmit_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_terminal_null_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_TERMINAL_NULL_REFERENCES"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_deltas_terminal_null_references,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| {
            emit_arena(
                &m.parasolid.parasolid_deltas_terminal_null_references,
                r,
                ns,
            )
        },
        len: |m| m.parasolid.parasolid_deltas_terminal_null_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_RECORD"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_deltas_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_records, r, ns),
        len: |m| m.parasolid.parasolid_deltas_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_tombstones",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_TOMBSTONE"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_deltas_tombstones, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_tombstones, r, ns),
        len: |m| m.parasolid.parasolid_deltas_tombstones.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_body_revisions",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_BODY_REVISION"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_deltas_body_revisions, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_body_revisions, r, ns),
        len: |m| m.parasolid.parasolid_deltas_body_revisions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_term_use_numeric_tails",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("TERM_USE_TAIL"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_deltas_term_use_numeric_tails,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_term_use_numeric_tails, r, ns),
        len: |m| m.parasolid.parasolid_deltas_term_use_numeric_tails.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_tagged_reference_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_TAGGED_REFERENCES"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_deltas_tagged_reference_lanes,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_tagged_reference_lanes, r, ns),
        len: |m| m.parasolid.parasolid_deltas_tagged_reference_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_reference_type_maps",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_REFERENCE_TYPE_MAP"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_deltas_reference_type_maps, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_reference_type_maps, r, ns),
        len: |m| m.parasolid.parasolid_deltas_reference_type_maps.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_reference_state_packets",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_REFERENCE_STATE"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_deltas_reference_state_packets,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_reference_state_packets, r, ns),
        len: |m| m.parasolid.parasolid_deltas_reference_state_packets.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_schema_reference_preambles",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_SCHEMA_REFERENCE_PREAMBLE"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_deltas_schema_reference_preambles,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| {
            emit_arena(
                &m.parasolid.parasolid_deltas_schema_reference_preambles,
                r,
                ns,
            )
        },
        len: |m| {
            m.parasolid
                .parasolid_deltas_schema_reference_preambles
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_reference_marker_packets",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_REFERENCE_MARKER"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_deltas_reference_marker_packets,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| {
            emit_arena(
                &m.parasolid.parasolid_deltas_reference_marker_packets,
                r,
                ns,
            )
        },
        len: |m| m.parasolid.parasolid_deltas_reference_marker_packets.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_type_150_state_packets",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_TYPE_150_STATE"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_deltas_type_150_state_packets,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_type_150_state_packets, r, ns),
        len: |m| m.parasolid.parasolid_deltas_type_150_state_packets.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_inline_schema_declarations",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_INLINE_SCHEMA"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_deltas_inline_schema_declarations,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| {
            emit_arena(
                &m.parasolid.parasolid_deltas_inline_schema_declarations,
                r,
                ns,
            )
        },
        len: |m| {
            m.parasolid
                .parasolid_deltas_inline_schema_declarations
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_inline_body_states",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_INLINE_BODY_STATE"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_deltas_inline_body_states, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_inline_body_states, r, ns),
        len: |m| m.parasolid.parasolid_deltas_inline_body_states.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_deltas_residual_spans",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("DELTAS_RESIDUAL"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_deltas_residual_spans, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_deltas_residual_spans, r, ns),
        len: |m| m.parasolid.parasolid_deltas_residual_spans.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_blend_surface_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("BLEND_SURF"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_blend_surface_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_blend_surface_records, r, ns),
        len: |m| m.parasolid.parasolid_blend_surface_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_blend_bound_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("BLEND_BOUND"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_blend_bound_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_blend_bound_records, r, ns),
        len: |m| m.parasolid.parasolid_blend_bound_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_offset_surface_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OFFSET_SURF"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_offset_surface_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_offset_surface_records, r, ns),
        len: |m| m.parasolid.parasolid_offset_surface_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_trimmed_curve_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("TRIMMED_CURVE"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_trimmed_curve_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_trimmed_curve_records, r, ns),
        len: |m| m.parasolid.parasolid_trimmed_curve_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_surface_curve_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("SP_CURVE"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_surface_curve_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_surface_curve_records, r, ns),
        len: |m| m.parasolid.parasolid_surface_curve_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_intersection_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: None,
            note: note_parasolid_parasolid_intersection_records,
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_intersection_records, r, ns),
        len: |m| m.parasolid.parasolid_intersection_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_term_use_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("term_use"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_term_use_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_term_use_records, r, ns),
        len: |m| m.parasolid.parasolid_term_use_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_support_uv_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("values"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_support_uv_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_support_uv_records, r, ns),
        len: |m| m.parasolid.parasolid_support_uv_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_chart_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("CHART_s"),
            note: |m, r, tag, a| note_per_stream(&m.parasolid.parasolid_chart_records, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_chart_records, r, ns),
        len: |m| m.parasolid.parasolid_chart_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_attribute_definitions",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ATTRIBUTE_DEFINITION"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_attribute_definitions, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_attribute_definitions, r, ns),
        len: |m| m.parasolid.parasolid_attribute_definitions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_field_names_records",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_field_names_records, r, ns),
        len: |m| m.parasolid.parasolid_field_names_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_attribute_field_names",
        exactness: Exactness::Derived,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_attribute_field_names, r, ns),
        len: |m| m.parasolid.parasolid_attribute_field_names.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_51"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_51_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_51_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_51_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_52_integer_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_52_INTEGERS"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_52_integer_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_52_integer_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_52_integer_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_53_double_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_53_DOUBLES"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_53_double_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_53_double_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_53_double_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_54_string_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_54_STRING"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_54_string_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_54_string_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_54_string_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_vector_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ATTRIBUTE_VECTOR_VALUES"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_vector_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_vector_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_vector_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_57_axis_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_57_AXES"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_57_axis_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_57_axis_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_57_axis_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_58_tag_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_58_TAGS"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_58_tag_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_58_tag_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_58_tag_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_62_unicode_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_62_UNICODE"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_62_unicode_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_62_unicode_records, r, ns),
        len: |m| m.parasolid.parasolid_entity_62_unicode_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_string_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_51_STRING_USE"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_51_string_uses, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_51_string_uses, r, ns),
        len: |m| m.parasolid.parasolid_entity_51_string_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_numeric_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("ENTITY_51_NUMERIC_USE"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_51_numeric_uses, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_51_numeric_uses, r, ns),
        len: |m| m.parasolid.parasolid_entity_51_numeric_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_entity_51_structured_uses",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("ENTITY_51_STRUCTURED_USE"),
            note: |m, r, tag, a| {
                note_per_stream(&m.parasolid.parasolid_entity_51_structured_uses, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_entity_51_structured_uses, r, ns),
        len: |m| m.parasolid.parasolid_entity_51_structured_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_attribute_class_uses",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: None,
            note: note_parasolid_parasolid_attribute_class_uses,
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_attribute_class_uses, r, ns),
        len: |m| m.parasolid.parasolid_attribute_class_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_attribute_field_uses",
        exactness: Exactness::Derived,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_attribute_field_uses, r, ns),
        len: |m| m.parasolid.parasolid_attribute_field_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "parasolid_topology_attribute_list_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("TOPOLOGY_ATTRIBUTE_LIST_REFERENCE"),
            note: |m, r, tag, a| {
                note_per_stream(
                    &m.parasolid.parasolid_topology_attribute_list_references,
                    r,
                    tag,
                    a,
                );
            },
        },
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
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: None,
            note: note_parasolid_parasolid_topology_attribute_class_uses,
        },
        emit: |m, r, ns| emit_arena(&m.parasolid.parasolid_topology_attribute_class_uses, r, ns),
        len: |m| m.parasolid.parasolid_topology_attribute_class_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_object_frames",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OFFSET_STORE_OBJECT_FRAME"),
            note: |m, r, tag, a| note_container(&m.features.data_block_object_frames, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.features.data_block_object_frames, r, ns),
        len: |m| m.features.data_block_object_frames.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "offset_store_named_points",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OFFSET_STORE_NAMED_POINT"),
            note: |m, r, tag, a| note_container(&m.features.offset_store_named_points, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.features.offset_store_named_points, r, ns),
        len: |m| m.features.offset_store_named_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_named_point_block_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("SKETCH_NAMED_POINT_BLOCK_USE"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_sketch_named_point_block_uses, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_named_point_block_uses, r, ns),
        len: |m| m.features.feature_sketch_named_point_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_preceding_named_point_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("SKETCH_PRECEDING_NAMED_POINT_USE"),
            note: |m, r, tag, a| {
                note_container(
                    &m.features.feature_sketch_preceding_named_point_uses,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_preceding_named_point_uses, r, ns),
        len: |m| m.features.feature_sketch_preceding_named_point_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_point_uses",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: None,
            note: note_features_feature_sketch_point_uses,
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_point_uses, r, ns),
        len: |m| m.features.feature_sketch_point_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_datum_csys_dependencies",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("SKETCH_DATUM_CSYS_DEPENDENCY"),
            note: |m, r, tag, a| {
                note_container(
                    &m.features.feature_sketch_datum_csys_dependencies,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_datum_csys_dependencies, r, ns),
        len: |m| m.features.feature_sketch_datum_csys_dependencies.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_input_block_identity_groups",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: None,
            note: note_features_feature_input_block_identity_groups,
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_input_block_identity_groups, r, ns),
        len: |m| m.features.feature_input_block_identity_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_abr_reference_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OFFSET_STORE_ABR_REFERENCE_LANE"),
            note: |m, r, tag, a| note_container(&m.om.data_block_abr_reference_lanes, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.data_block_abr_reference_lanes, r, ns),
        len: |m| m.om.data_block_abr_reference_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "segment_om_links",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("UG_PART_SEGMENT_OM_LINK"),
            note: |m, r, tag, a| note_container(&m.segments.segment_om_links, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.segments.segment_om_links, r, ns),
        len: |m| m.segments.segment_om_links.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_record_areas",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_RECORD_AREA"),
            note: |m, r, tag, a| note_container(&m.om.om_record_areas, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.om_record_areas, r, ns),
        len: |m| m.om.om_record_areas.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_audit_trail_rows",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_AUDIT_TRAIL_ROW"),
            note: |m, r, tag, a| note_container(&m.om.audit_trail_rows, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.audit_trail_rows, r, ns),
        len: |m| m.om.audit_trail_rows.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_operation_state_journal_groups",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_OPERATION_STATE_JOURNAL_GROUP"),
            note: |m, r, tag, a| note_container(&m.om.operation_state_journal_groups, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.operation_state_journal_groups, r, ns),
        len: |m| m.om.operation_state_journal_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_operation_state_counters",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_OPERATION_STATE_COUNTER"),
            note: |m, r, tag, a| note_container(&m.om.operation_state_counters, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.operation_state_counters, r, ns),
        len: |m| m.om.operation_state_counters.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_roll_forward_state_groups",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_ROLL_FORWARD_STATE_GROUP"),
            note: |m, r, tag, a| {
                for table in &m.om.operation_state_groups {
                    note_container(table.groups(), r, tag, a);
                }
            },
        },
        emit: |m, r, ns| {
            let groups =
                m.om.operation_state_groups
                    .iter()
                    .flat_map(OmRollForwardStateTable::groups)
                    .collect::<Vec<_>>();
            emit_arena(&groups, r, ns)
        },
        len: |m| {
            m.om.operation_state_groups
                .iter()
                .map(|table| table.groups().len())
                .sum()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_operation_state_messages",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_OPERATION_STATE_MESSAGE"),
            note: |m, r, tag, a| note_container(&m.om.operation_state_messages, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.operation_state_messages, r, ns),
        len: |m| m.om.operation_state_messages.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_operation_state_statuses",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_OPERATION_STATE_STATUS"),
            note: |m, r, tag, a| note_container(&m.om.operation_state_statuses, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.operation_state_statuses, r, ns),
        len: |m| m.om.operation_state_statuses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "om_operation_state_slot_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_OPERATION_STATE_SLOT_LANE"),
            note: |m, r, tag, a| note_container(&m.om.operation_state_slot_lanes, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.operation_state_slot_lanes, r, ns),
        len: |m| m.om.operation_state_slot_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_labels",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_OPERATION_LABEL"),
            note: |m, r, tag, a| note_container(&m.features.feature_operation_labels, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_labels, r, ns),
        len: |m| m.features.feature_operation_labels.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_records",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("FEATURE_SKETCH_RECORD"),
            note: |m, r, tag, a| note_container(&m.features.feature_sketch_records, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_records, r, ns),
        len: |m| m.features.feature_sketch_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_fixed_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_SKETCH_FIXED_PAIR"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_sketch_payload_fixed_pairs, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_fixed_pairs, r, ns),
        len: |m| m.features.feature_sketch_payload_fixed_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_mixed_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_SKETCH_MIXED_PAIR"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_sketch_payload_mixed_pairs, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_mixed_pairs, r, ns),
        len: |m| m.features.feature_sketch_payload_mixed_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_fixed_points",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("FEATURE_SKETCH_FIXED_POINT"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_sketch_fixed_points, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_fixed_points, r, ns),
        len: |m| m.features.feature_sketch_fixed_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_OPERATION_RECORD"),
            note: |m, r, tag, a| note_container(&m.features.feature_operation_records, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_records, r, ns),
        len: |m| m.features.feature_operation_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_unlabeled_operation_records",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_UNLABELED_OPERATION_RECORD"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_unlabeled_operation_records, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_unlabeled_operation_records, r, ns),
        len: |m| m.features.feature_unlabeled_operation_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_unlabeled_operation_body_writes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_UNLABELED_OPERATION_BODY_WRITE"),
            note: |m, r, tag, a| {
                note_container(
                    &m.features.feature_unlabeled_operation_body_writes,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_unlabeled_operation_body_writes, r, ns),
        len: |m| m.features.feature_unlabeled_operation_body_writes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_writes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_OPERATION_BODY_WRITE"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_operation_body_writes, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_writes, r, ns),
        len: |m| m.features.feature_operation_body_writes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_tagged_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_OPERATION_TAGGED_REFERENCE"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_operation_tagged_references, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_tagged_references, r, ns),
        len: |m| m.features.feature_operation_tagged_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_data_block_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_OPERATION_DATA_BLOCK_REFERENCE"),
            note: |m, r, tag, a| {
                note_container(
                    &m.features.feature_operation_data_block_references,
                    r,
                    tag,
                    a,
                );
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_data_block_references, r, ns),
        len: |m| m.features.feature_operation_data_block_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_common_frames",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_OPERATION_COMMON_FRAME"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_operation_common_frames, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_common_frames, r, ns),
        len: |m| m.features.feature_operation_common_frames.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_terminal_discriminators",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_terminal_discriminators, r, ns),
        len: |m| m.features.feature_operation_terminal_discriminators.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_terminal_frames",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_OPERATION_TERMINAL_FRAME"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_operation_terminal_frames, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_terminal_frames, r, ns),
        len: |m| m.features.feature_operation_terminal_frames.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_state_journal_uses",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("FEATURE_OPERATION_STATE_JOURNAL_USE"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_operation_state_journal_uses, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_state_journal_uses, r, ns),
        len: |m| m.features.feature_operation_state_journal_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_payload_strings",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_PAYLOAD_STRING"),
            note: |m, r, tag, a| note_container(&m.features.feature_payload_strings, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_payload_strings, r, ns),
        len: |m| m.features.feature_payload_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_BODY_REFERENCE"),
            note: |m, r, tag, a| note_container(&m.features.feature_body_references, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_body_references, r, ns),
        len: |m| m.features.feature_body_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_reference_occurrences",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_BODY_REFERENCE_OCCURRENCE"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_body_reference_occurrences, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_body_reference_occurrences, r, ns),
        len: |m| m.features.feature_body_reference_occurrences.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_blocks",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_INPUT_BLOCK"),
            note: |m, r, tag, a| note_container(&m.features.feature_input_blocks, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_input_blocks, r, ns),
        len: |m| m.features.feature_input_blocks.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_boolean_operations",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("FEATURE_BOOLEAN_OPERATION"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_boolean_operations, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_boolean_operations, r, ns),
        len: |m| m.features.feature_boolean_operations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "expression_declarations",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("EXPRESSION_DECLARATION"),
            note: |m, r, tag, a| note_container(&m.om.expression_declarations, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.expression_declarations, r, ns),
        len: |m| m.om.expression_declarations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_forms",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_DATA_BLOCK_CONTROL_FORM"),
            note: |m, r, tag, a| note_container(&m.om.data_block_control_forms, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_forms, r, ns),
        len: |m| m.om.data_block_control_forms.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_values",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_DATA_BLOCK_CONTROL_VALUE"),
            note: |m, r, tag, a| note_container(&m.om.data_block_control_values, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_values, r, ns),
        len: |m| m.om.data_block_control_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_class_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_DATA_BLOCK_CONTROL_CLASS_REFERENCE"),
            note: |m, r, tag, a| {
                note_container(&m.om.data_block_control_class_references, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_class_references, r, ns),
        len: |m| m.om.data_block_control_class_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_index_values",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_DATA_BLOCK_CONTROL_INDEX_VALUE"),
            note: |m, r, tag, a| note_container(&m.om.data_block_control_index_values, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_index_values, r, ns),
        len: |m| m.om.data_block_control_index_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_DATA_BLOCK_CONTROL_REFERENCE"),
            note: |m, r, tag, a| note_container(&m.om.data_block_control_references, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_references, r, ns),
        len: |m| m.om.data_block_control_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_control_handle_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_DATA_BLOCK_CONTROL_HANDLE_PAIR"),
            note: |m, r, tag, a| note_container(&m.om.data_block_control_handle_pairs, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.data_block_control_handle_pairs, r, ns),
        len: |m| m.om.data_block_control_handle_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "object_record_handle_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_OBJECT_RECORD_HANDLE_PAIR"),
            note: |m, r, tag, a| note_container(&m.om.object_record_handle_pairs, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.object_record_handle_pairs, r, ns),
        len: |m| m.om.object_record_handle_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupA {
            tag: Some("OM_DATA_BLOCK_REFERENCE"),
            note: |m, r, tag, a| note_container(&m.om.data_block_references, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.data_block_references, r, ns),
        len: |m| m.om.data_block_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_parameter_bindings",
        exactness: Exactness::Derived,
        phase: Phase::GroupA {
            tag: Some("FEATURE_PARAMETER_BINDING"),
            note: |m, r, tag, a| {
                note_container(&m.features.feature_parameter_bindings, r, tag, a);
            },
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_parameter_bindings, r, ns),
        len: |m| m.features.feature_parameter_bindings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_parameter_uses",
        exactness: Exactness::Derived,
        phase: Phase::GroupB {
            tag: None,
            note: note_features_feature_parameter_uses,
        },
        emit: |m, r, ns| emit_arena(&m.features.feature_parameter_uses, r, ns),
        len: |m| m.features.feature_parameter_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "store_headers",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupB {
            tag: Some("OM_STORE_VERSION"),
            note: |m, r, tag, a| note_container(&m.om.store_headers, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.store_headers, r, ns),
        len: |m| m.om.store_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_references",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupB {
            tag: Some("EXTREFSTREAM_STRING"),
            note: |m, r, tag, a| note_container(&m.om.external_references, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.om.external_references, r, ns),
        len: |m| m.om.external_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "fast_load_component_prototypes",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupB {
            tag: Some("FAST_LOAD_COMPONENT_PROTOTYPE"),
            note: |m, r, tag, a| note_container(&m.structure.prototypes, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.structure.prototypes, r, ns),
        len: |m| m.structure.prototypes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "fast_load_component_occurrences",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupB {
            tag: Some("FAST_LOAD_COMPONENT_OCCURRENCE"),
            note: |m, r, tag, a| note_container(m.structure.occurrences.as_slice(), r, tag, a),
        },
        emit: |m, r, ns| {
            if !m.structure.occurrences.as_slice().is_empty() {
                ns.set_arena_from(r.arena, m.structure.occurrences.wire_records())?;
            }
            Ok(())
        },
        len: |m| m.structure.occurrences.as_slice().len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "fast_load_component_uuids",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupB {
            tag: Some("FAST_LOAD_COMPONENT_UUID"),
            note: |m, r, tag, a| note_container(&m.structure.uuids, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.structure.uuids, r, ns),
        len: |m| m.structure.uuids.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "fast_load_component_object_groups",
        exactness: Exactness::Derived,
        phase: Phase::GroupB {
            tag: Some("FAST_LOAD_COMPONENT_OBJECT_GROUP"),
            note: |m, r, tag, a| note_container(&m.structure.object_groups, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.structure.object_groups, r, ns),
        len: |m| m.structure.object_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "saved_toggle_streams",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupB {
            tag: Some("SAVED_TOGGLE_STREAM"),
            note: |m, r, tag, a| note_container(&m.toggle.streams, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.toggle.streams, r, ns),
        len: |m| m.toggle.streams.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "saved_toggle_entries",
        exactness: Exactness::ByteExact,
        phase: Phase::GroupB {
            tag: Some("SAVED_TOGGLE_ENTRY"),
            note: |m, r, tag, a| note_container(&m.toggle.entries, r, tag, a),
        },
        emit: |m, r, ns| emit_arena(&m.toggle.entries, r, ns),
        len: |m| m.toggle.entries.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_records",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_records, r, ns),
        len: |m| m.om.external_reference_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "material_texture_assets",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.material_texture_assets, r, ns),
        len: |m| m.om.material_texture_assets.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "material_texture_catalog_entries",
        exactness: Exactness::Derived,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.material_texture_catalog_entries, r, ns),
        len: |m| m.om.material_texture_catalog_entries.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_body_segment_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_body_segment_uses, r, ns),
        len: |m| m.features.feature_body_segment_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_operation_body_image_segment_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_image_segment_uses, r, ns),
        len: |m| m.features.feature_operation_body_image_segment_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_operation_body_partition_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_partition_uses, r, ns),
        len: |m| m.features.feature_operation_body_partition_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_operation_body_identity_segment_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| {
            emit_arena(
                &m.features.feature_operation_body_identity_segment_uses,
                r,
                ns,
            )
        },
        len: |m| {
            m.features
                .feature_operation_body_identity_segment_uses
                .len()
        },
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_body_write_group_partition_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_body_write_group_partition_uses, r, ns),
        len: |m| m.features.feature_body_write_group_partition_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_body_data_block_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_body_data_block_uses, r, ns),
        len: |m| m.features.feature_body_data_block_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_symbolic_threads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_symbolic_threads, r, ns),
        len: |m| m.features.feature_symbolic_threads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_threaded_hole_templates",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_threaded_hole_templates, r, ns),
        len: |m| m.features.feature_threaded_hole_templates.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_templates",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_simple_hole_templates, r, ns),
        len: |m| m.features.feature_simple_hole_templates.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_repeated_scalar_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_simple_hole_repeated_scalar_lanes, r, ns),
        len: |m| m.features.feature_simple_hole_repeated_scalar_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_simple_hole_repeated_scalar_lane_block_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
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
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_simple_hole_construction_groups, r, ns),
        len: |m| m.features.feature_simple_hole_construction_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_hole_package_construction_group_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| {
            emit_arena(
                &m.features.feature_hole_package_construction_group_lanes,
                r,
                ns,
            )
        },
        len: |m| {
            m.features
                .feature_hole_package_construction_group_lanes
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_hole_package_construction_group_uses",
        exactness: Exactness::Derived,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| {
            emit_arena(
                &m.features.feature_hole_package_construction_group_uses,
                r,
                ns,
            )
        },
        len: |m| {
            m.features
                .feature_hole_package_construction_group_uses
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_csys_constructions",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_constructions, r, ns),
        len: |m| m.features.feature_datum_csys_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_csys_column_row_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_column_row_uses, r, ns),
        len: |m| m.features.feature_datum_csys_column_row_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_payloads, r, ns),
        len: |m| m.features.feature_datum_csys_payloads.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_scalar_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_payload_scalar_pairs, r, ns),
        len: |m| m.features.feature_datum_csys_payload_scalar_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_fixed_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_payload_fixed_pairs, r, ns),
        len: |m| m.features.feature_datum_csys_payload_fixed_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_payload_scalars",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_payload_scalars, r, ns),
        len: |m| m.features.feature_datum_csys_payload_scalars.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_descriptors",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_descriptors, r, ns),
        len: |m| m.features.feature_datum_csys_descriptors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_csys_block_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_csys_block_uses, r, ns),
        len: |m| m.features.feature_datum_csys_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_headers",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_headers, r, ns),
        len: |m| m.features.feature_datum_plane_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_block_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_block_uses, r, ns),
        len: |m| m.features.feature_datum_plane_block_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_payloads, r, ns),
        len: |m| m.features.feature_datum_plane_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_datum_plane_payload_scalar_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_payload_scalar_pairs, r, ns),
        len: |m| m.features.feature_datum_plane_payload_scalar_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_plane_descriptors",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_descriptors, r, ns),
        len: |m| m.features.feature_datum_plane_descriptors.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_datum_plane_csys_identity_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_datum_plane_csys_identity_uses, r, ns),
        len: |m| m.features.feature_datum_plane_csys_identity_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_references, r, ns),
        len: |m| m.features.feature_sketch_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_projected_curve_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_projected_curve_references, r, ns),
        len: |m| m.features.feature_projected_curve_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_projected_curve_construction_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
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
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
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
        arena: "feature_fset_reference_graphs",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_fset_reference_graphs, r, ns),
        len: |m| m.features.feature_fset_reference_graphs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_fset_construction_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_fset_construction_payloads, r, ns),
        len: |m| m.features.feature_fset_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_delete_reference_fields",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_delete_reference_fields, r, ns),
        len: |m| m.features.feature_delete_reference_fields.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_delete_construction_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_delete_construction_payloads, r, ns),
        len: |m| m.features.feature_delete_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_references, r, ns),
        len: |m| m.features.feature_pattern_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_counted_reference_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_counted_reference_lanes, r, ns),
        len: |m| m.features.feature_pattern_counted_reference_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_construction_payloads, r, ns),
        len: |m| m.features.feature_pattern_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_strings",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_construction_strings, r, ns),
        len: |m| m.features.feature_pattern_construction_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_construction_fixed_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_construction_fixed_lanes, r, ns),
        len: |m| m.features.feature_pattern_construction_fixed_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_pattern_transform_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_pattern_transform_lanes, r, ns),
        len: |m| m.features.feature_pattern_transform_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_multi_instance_output_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_multi_instance_output_lanes, r, ns),
        len: |m| m.features.feature_multi_instance_output_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_identical_instance_output_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_identical_instance_output_lanes, r, ns),
        len: |m| m.features.feature_identical_instance_output_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_point_construction_headers",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_point_construction_headers, r, ns),
        len: |m| m.features.feature_point_construction_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_point_construction_scalar_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_point_construction_scalar_lanes, r, ns),
        len: |m| m.features.feature_point_construction_scalar_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_references, r, ns),
        len: |m| m.features.feature_draft_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_index_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_index_lanes, r, ns),
        len: |m| m.features.feature_draft_construction_index_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_payloads, r, ns),
        len: |m| m.features.feature_draft_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_graph_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_graph_payloads, r, ns),
        len: |m| m.features.feature_draft_construction_graph_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_fixed_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_fixed_lanes, r, ns),
        len: |m| m.features.feature_draft_construction_fixed_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_binary32_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_binary32_lanes, r, ns),
        len: |m| m.features.feature_draft_construction_binary32_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_graph_strings",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_graph_strings, r, ns),
        len: |m| m.features.feature_draft_construction_graph_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_draft_construction_identity_frames",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
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
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_draft_construction_terminal_lanes, r, ns),
        len: |m| m.features.feature_draft_construction_terminal_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_references, r, ns),
        len: |m| m.features.feature_surface_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_payloads, r, ns),
        len: |m| m.features.feature_surface_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_scalar_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_scalar_pairs, r, ns),
        len: |m| m.features.feature_surface_construction_scalar_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_strings",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_strings, r, ns),
        len: |m| m.features.feature_surface_construction_strings.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_surface_construction_branches",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_surface_construction_branches, r, ns),
        len: |m| m.features.feature_surface_construction_branches.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_swp104_leading_branches",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_swp104_leading_branches, r, ns),
        len: |m| m.features.feature_swp104_leading_branches.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_thru_curve_construction_branch_groups",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| {
            emit_arena(
                &m.features.feature_thru_curve_construction_branch_groups,
                r,
                ns,
            )
        },
        len: |m| {
            m.features
                .feature_thru_curve_construction_branch_groups
                .len()
        },
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_thru_curve_construction_envelopes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_thru_curve_construction_envelopes, r, ns),
        len: |m| m.features.feature_thru_curve_construction_envelopes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_profile_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_profile_references, r, ns),
        len: |m| m.features.feature_extrude_profile_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_payload_headers",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_payload_headers, r, ns),
        len: |m| m.features.feature_extrude_payload_headers.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_scalar_triples",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_scalar_triples, r, ns),
        len: |m| m.features.feature_operation_body_scalar_triples.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_members",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_members, r, ns),
        len: |m| m.features.feature_operation_body_members.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_operands",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_operands, r, ns),
        len: |m| m.features.feature_operation_body_operands.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_11_continuations",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_11_continuations, r, ns),
        len: |m| m.features.feature_operation_body_11_continuations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_operation_body_reference_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_operation_body_reference_lanes, r, ns),
        len: |m| m.features.feature_operation_body_reference_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_construction_profiles",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_construction_profiles, r, ns),
        len: |m| m.features.feature_extrude_construction_profiles.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_payload_32_branches",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_payload_32_branches, r, ns),
        len: |m| m.features.feature_extrude_payload_32_branches.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_extrude_32_constructions",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_extrude_32_constructions, r, ns),
        len: |m| m.features.feature_extrude_32_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_construction_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_construction_references, r, ns),
        len: |m| m.features.feature_block_construction_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_constructions",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_constructions, r, ns),
        len: |m| m.features.feature_block_constructions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_construction_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_construction_payloads, r, ns),
        len: |m| m.features.feature_block_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_scalars",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_scalars, r, ns),
        len: |m| m.features.feature_block_payload_scalars.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_names",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_names, r, ns),
        len: |m| m.features.feature_block_payload_names.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_named_records",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_named_records, r, ns),
        len: |m| m.features.feature_block_payload_named_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_points",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_points, r, ns),
        len: |m| m.features.feature_block_payload_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_payload_point_groups",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_payload_point_groups, r, ns),
        len: |m| m.features.feature_block_payload_point_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_block_dimensions",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_block_dimensions, r, ns),
        len: |m| m.features.feature_block_dimensions.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_construction_inputs",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_construction_inputs, r, ns),
        len: |m| m.features.feature_sketch_construction_inputs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_construction_payloads",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_construction_payloads, r, ns),
        len: |m| m.features.feature_sketch_construction_payloads.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_coordinate_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_coordinate_pairs, r, ns),
        len: |m| m.features.feature_sketch_payload_coordinate_pairs.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_scalars",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_scalars, r, ns),
        len: |m| m.features.feature_sketch_payload_scalars.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_scalar_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_scalar_lanes, r, ns),
        len: |m| m.features.feature_sketch_payload_scalar_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_names",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_names, r, ns),
        len: |m| m.features.feature_sketch_payload_names.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_payload_named_records",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_payload_named_records, r, ns),
        len: |m| m.features.feature_sketch_payload_named_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_points",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_points, r, ns),
        len: |m| m.features.feature_sketch_points.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "feature_sketch_point_groups",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_sketch_point_groups, r, ns),
        len: |m| m.features.feature_sketch_point_groups.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "expressions",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.expressions, r, ns),
        len: |m| m.om.expressions.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "class_definitions",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.classes, r, ns),
        len: |m| m.om.classes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "field_definitions",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.fields, r, ns),
        len: |m| m.om.fields.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "object_records",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.object_records, r, ns),
        len: |m| m.om.object_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "rmfastload_object_id_tables",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.rmfastload_object_id_tables, r, ns),
        len: |m| m.om.rmfastload_object_id_tables.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "rmfastload_object_ids",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.rmfastload_object_ids, r, ns),
        len: |m| m.om.rmfastload_object_ids.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_blocks",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.data_blocks, r, ns),
        len: |m| m.om.data_blocks.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_counted_index_lanes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.data_block_counted_index_lanes, r, ns),
        len: |m| m.om.data_block_counted_index_lanes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "data_block_index_rows",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.data_block_index_rows, r, ns),
        len: |m| m.om.data_block_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_linked_index_rows",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.data_block_linked_index_rows, r, ns),
        len: |m| m.om.data_block_linked_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_target_index_rows",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.data_block_target_index_rows, r, ns),
        len: |m| m.om.data_block_target_index_rows.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "rm_creation_display_data_relations",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.rm_creation_display_data_relations, r, ns),
        len: |m| m.om.rm_creation_display_data_relations.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "part_color_tables",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.part_color_tables, r, ns),
        len: |m| m.om.part_color_tables.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "part_color_definitions",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.part_color_definitions, r, ns),
        len: |m| m.om.part_color_definitions.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "rm_display_color_assignments",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.rm_display_color_assignments, r, ns),
        len: |m| m.om.rm_display_color_assignments.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "data_block_column_index_tables",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.data_block_column_index_tables, r, ns),
        len: |m| m.om.data_block_column_index_tables.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_column_row_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_input_column_row_uses, r, ns),
        len: |m| m.features.feature_input_column_row_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "feature_input_column_targets",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.features.feature_input_column_targets, r, ns),
        len: |m| m.features.feature_input_column_targets.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "string_values",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.string_values, r, ns),
        len: |m| m.om.string_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "object_uuid_values",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.object_uuid_values, r, ns),
        len: |m| m.om.object_uuid_values.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "object_references",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.object_references, r, ns),
        len: |m| m.om.object_references.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "persistent_handles",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.persistent_handles, r, ns),
        len: |m| m.om.persistent_handles.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "configurations",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.configurations, r, ns),
        len: |m| m.om.configurations.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "configuration_attribute_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.configuration_attribute_uses, r, ns),
        len: |m| m.om.configuration_attribute_uses.len(),
        counts_toward_emptiness: false,
    },
    CatalogueRow {
        arena: "part_attributes",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.part_attributes, r, ns),
        len: |m| m.om.part_attributes.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_indexed_records",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_indexed_records, r, ns),
        len: |m| m.om.external_reference_indexed_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_empty_records",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_empty_records, r, ns),
        len: |m| m.om.external_reference_empty_records.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_tail_reference_pairs",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_tail_reference_pairs, r, ns),
        len: |m| m.om.external_reference_tail_reference_pairs.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_record_string_uses",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_record_string_uses, r, ns),
        len: |m| m.om.external_reference_record_string_uses.len(),
        counts_toward_emptiness: true,
    },
    CatalogueRow {
        arena: "external_reference_record_children",
        exactness: Exactness::ByteExact,
        phase: Phase::ArenaOnly,
        emit: |m, r, ns| emit_arena(&m.om.external_reference_record_children, r, ns),
        len: |m| m.om.external_reference_record_children.len(),
        counts_toward_emptiness: true,
    },
];

/// Executable catalogue over the frozen NX family table.
pub(crate) const NATIVE_CATALOGUE: Catalogue<
    'static,
    NativeModel,
    AnnotationBuilder,
    NativeNamespace,
    Exactness,
> = Catalogue::new(CATALOGUE);

#[cfg(test)]
mod tests {
    #[test]
    fn class_use_annotation_survives_absent_entity_record() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        use cadmpeg_ir::native::catalogue::NotePhase;
        let bytes = crate::test_support::prt_with_partition(
            &crate::test_support::parasolid_entity_records_stream(),
        );
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::decode::scan(&ctx, root).unwrap();
        let mut parsed = crate::native::ParsedStreams::parse(&scan);
        let mut model = super::NativeModel::extract(
            &ctx,
            root,
            &scan.container,
            &scan.streams,
            &mut parsed,
            None,
        )
        .unwrap();
        assert!(!model.parasolid.parasolid_attribute_class_uses.is_empty());
        let id = model.parasolid.parasolid_attribute_class_uses[0].id.clone();
        model.parasolid.parasolid_entity_51_records = Vec::new();
        let mut annotations = super::AnnotationBuilder::new();
        super::NATIVE_CATALOGUE.note_phase(NotePhase::GroupA, &model, &mut annotations);
        assert!(annotations.annotations().exactness().contains_key(&id));
    }
}
