// SPDX-License-Identifier: Apache-2.0
//! STEP target resolution and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, ArenaDisposition, ArenaDispositions, Consumption,
    EncodeInput, ExportBody, NativeDisposition, NativeDispositions, WritePath,
};

use cadmpeg_ir::report::loss::LossNote;

use crate::export::write_step_outcome;
use crate::loss::StepLossCode;
use crate::options::StepSchema;
use crate::StepCodec;

/// Why this writer cannot reproduce a source schema outside
/// [`StepSchema::TARGETS`].
///
/// STEP has no retained-image path, and every schema this writer emits stamps
/// object-identifier arcs, so an edition-unspecified or unrecognized
/// declaration cannot be written back.
const OFF_CATALOG_SOURCE_REASON: &str =
    "the semantic writer cannot synthesize it, and writing another schema would change what the \
     file declares; name a target to choose one";

/// Resolve the request against the source and synthesize the selected schema.
pub(crate) fn plan(
    codec: &StepCodec,
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let Some(index) = resolved.index() else {
        return Err(resolved.unavailable(OFF_CATALOG_SOURCE_REASON));
    };
    let schema = StepSchema::from_target_index(index).ok_or_else(|| {
        resolved.unavailable("the resolved target index is outside the STEP schema catalog")
    })?;
    let mut bytes = Vec::new();
    let outcome = write_step_outcome(input.ir, &mut bytes, schema, &codec.options)?;
    let coverage = coverage(schema, &outcome.losses);
    let mut losses = outcome.losses;
    if let Some(message) = resolved.displacement_message() {
        losses.push(StepLossCode::SourceDialectDisplaced.note(message));
    }
    Ok(ExportBody {
        native_dispositions: NativeDispositions::for_namespace(
            "step",
            crate::reader::NATIVE_ARENAS,
            NativeDisposition::Dropped,
        ),
        synthesis_cause: None,
        bytes,
        census: outcome.census,
        write_path: WritePath::Synthesized {
            // STEP has no retained image: a provided fidelity is never replayed.
            consumption: Consumption::NotConsumed,
        },
        coverage: ArenaCoverage::Declared(coverage),
        losses,
        notes: outcome.notes,
    })
}

/// What the STEP writer does with each model arena.
///
/// `Builder::note_unrepresented` in `export.rs` charges a specific loss for
/// every arena or record the graph does not carry. Arenas charged as a whole
/// are `Dropped`.
fn coverage(schema: StepSchema, losses: &[LossNote]) -> ArenaDispositions {
    use ArenaDisposition::{Dropped, Reported, Written};
    let ap242 = |supported: bool| if supported { Written } else { Reported };
    let mut coverage = ArenaDispositions {
        bodies: Written,
        regions: Written,
        shells: Written,
        faces: Written,
        loops: Written,
        coedges: Written,
        edges: Written,
        vertices: Written,
        points: Written,
        surfaces: Written,
        curves: Written,
        subds: Dropped,
        pcurves: Written,
        procedural_surfaces: Written,
        procedural_curves: Written,
        assets: Dropped,
        features: Dropped,
        feature_input_topologies: Dropped,
        feature_result_topologies: Dropped,
        configurations: Dropped,
        parameters: Dropped,
        sketches: Dropped,
        sketch_entities: Dropped,
        sketch_constraints: Dropped,
        spatial_sketches: Dropped,
        spatial_sketch_entities: Dropped,
        spatial_sketch_constraints: Dropped,
        spreadsheets: Dropped,
        product_definitions: Written,
        occurrences: Written,
        assembly_joints: Dropped,
        drawings: Dropped,
        semantic_annotations: Dropped,
        presentation_documents: Dropped,
        view_presentations: Dropped,
        tessellations: ap242(schema.supports_tessellation()),
        appearances: Written,
        appearance_bindings: Written,
        attributes: Dropped,
        pmi: ap242(schema.supports_semantic_pmi()),
        presentation_layers: Written,
    };
    report_reductions(&mut coverage, losses);
    coverage
}

/// Preserve Written for complete encodings, and describe actual reductions by
/// arena. Aggregate geometry/topology losses cover their whole carrier family:
/// their structured code does not identify individual arena populations.
fn report_reductions(coverage: &mut ArenaDispositions, losses: &[LossNote]) {
    use ArenaDisposition::Reported;
    let has = |codes: &[StepLossCode]| {
        losses
            .iter()
            .any(|loss| codes.iter().any(|code| loss.code == code.kind()))
    };
    if has(&[
        StepLossCode::BodyWithoutProductRepresentation,
        StepLossCode::SurfaceShellWireTopologyOmitted,
        StepLossCode::WireShellFreeVertices,
        StepLossCode::TopologyUnreachableFromRegion,
        StepLossCode::CurvelessEdgeOmitted,
        StepLossCode::UnknownSurfaceFaceOmitted,
        StepLossCode::RegionNoShellList,
        StepLossCode::WireRegionNoConnectedEdgeSet,
        StepLossCode::WireRegionMissingShell,
        StepLossCode::RegionNoWritableOuterShell,
        StepLossCode::RegionOmittedVoidShell,
        StepLossCode::ShellOmittedOuterFace,
        StepLossCode::ShellOmittedUnclassifiedFace,
        StepLossCode::FaceOmittedOuterLoop,
        StepLossCode::FaceOmittedInnerLoop,
        StepLossCode::FaceOmittedUnclassifiedLoop,
        StepLossCode::FaceNoWritableBounds,
        StepLossCode::LoopRecordMissing,
        StepLossCode::LoopVertexMissing,
        StepLossCode::LoopCoedgeRecordMissing,
        StepLossCode::LoopEdgeNotWritable,
        StepLossCode::LoopNoWritableEdges,
        StepLossCode::LoopDuplicateCoedge,
        StepLossCode::LoopCoedgeMissingForOrder,
        StepLossCode::LoopEdgeMissingForOrder,
        StepLossCode::EdgeNo3dCurve,
        StepLossCode::EdgeCurveNotWritable,
        StepLossCode::EdgeCurveUnsupported,
        StepLossCode::LoopNoContinuousOrdering,
        StepLossCode::EdgeRecordMissing,
        StepLossCode::EdgeStartVertexMissing,
        StepLossCode::EdgeEndVertexMissing,
    ]) {
        coverage.bodies = Reported;
        coverage.regions = Reported;
        coverage.shells = Reported;
        coverage.faces = Reported;
        coverage.loops = Reported;
        coverage.coedges = Reported;
        coverage.edges = Reported;
        coverage.vertices = Reported;
    }
    if has(&[
        StepLossCode::GeometryCarrierNotWritten,
        StepLossCode::SourceAssociationOmitted,
    ]) {
        coverage.points = Reported;
        coverage.curves = Reported;
        coverage.surfaces = Reported;
    }
    if has(&[
        StepLossCode::AnalyticSurfaceNormalized,
        StepLossCode::EllipticalConeReduced,
        StepLossCode::UnknownSurfaceFaceOmitted,
    ]) {
        coverage.surfaces = Reported;
    }
    if has(&[
        StepLossCode::ProceduralReducedToCarrier,
        StepLossCode::ProceduralDefinitionNotWritten,
    ]) {
        coverage.procedural_curves = Reported;
        coverage.procedural_surfaces = Reported;
    }
    if has(&[
        StepLossCode::PcurveCarrierUnwritable,
        StepLossCode::CoedgePcurveNativeMetadata,
    ]) {
        coverage.pcurves = Reported;
    }
    if has(&[
        StepLossCode::PcurveCarrierUnwritable,
        StepLossCode::CoedgePcurveNoGeometry,
        StepLossCode::CoedgePcurveNativeMetadata,
        StepLossCode::PcurveUseNativeMetadata,
        StepLossCode::CoedgeUseCurveNotRepresented,
    ]) {
        coverage.coedges = Reported;
        coverage.loops = Reported;
    }
    if has(&[StepLossCode::TopologyMetadataNotRepresented]) {
        coverage.faces = Reported;
        coverage.edges = Reported;
        coverage.vertices = Reported;
    }
    if has(&[StepLossCode::DisplayColorUnstyled]) {
        coverage.faces = Reported;
    }
    if has(&[StepLossCode::NonManifoldEdgeSplit]) {
        coverage.edges = Reported;
    }
    if has(&[
        StepLossCode::BodyNonRigidTransform,
        StepLossCode::HiddenBodyVisibilityUnsupported,
        StepLossCode::HiddenBodyOmitted,
        StepLossCode::WireBodyTransparencyOmitted,
        StepLossCode::DisplayColorUnstyled,
    ]) {
        coverage.bodies = Reported;
    }
    if has(&[
        StepLossCode::ProductNonPartKind,
        StepLossCode::ProductBomPropertyOmitted,
        StepLossCode::AssemblyGraphInvalid,
    ]) {
        coverage.product_definitions = Reported;
    }
    if has(&[
        StepLossCode::AssemblyGraphInvalid,
        StepLossCode::RootOccurrencePlacementNotRepresentable,
        StepLossCode::OccurrenceUnresolvedParent,
        StepLossCode::OccurrenceNoLocalProduct,
        StepLossCode::OccurrencePlacementNotRigid,
        StepLossCode::AssemblyOccurrenceOmittedNoParentProduct,
        StepLossCode::OccurrenceExternalProduct,
        StepLossCode::OccurrenceNoWritableProduct,
        StepLossCode::OccurrenceMetadataOmitted,
    ]) {
        coverage.occurrences = Reported;
    }
    if has(&[
        StepLossCode::TessellationRequiresAp242,
        StepLossCode::TessellationFeatureEdges,
        StepLossCode::TessellationCornerNormals,
        StepLossCode::TessellationTriangleGroups,
        StepLossCode::TessellationTextureAssignments,
        StepLossCode::TessellationInvalidCardinality,
        StepLossCode::TessellationBodyLinkUnwritable,
        StepLossCode::TessellationMetadataReduced,
        StepLossCode::SourceAssociationOmitted,
    ]) {
        coverage.tessellations = Reported;
    }
    if has(&[
        StepLossCode::HiddenPmiVisibilityUnsupported,
        StepLossCode::PmiAnnotationNotWritten,
    ]) {
        coverage.pmi = Reported;
    }
    if has(&[
        StepLossCode::AppearanceReducedToBaseColor,
        StepLossCode::DisplayColorUnstyled,
        StepLossCode::HiddenAppearanceVisibilityUnsupported,
    ]) {
        coverage.appearances = Reported;
    }
    if has(&[
        StepLossCode::AppearanceBindingMissingAsset,
        StepLossCode::AppearanceBindingNoBaseColor,
        StepLossCode::AppearanceBindingTargetConflict,
        StepLossCode::AppearanceBindingMetadataReduced,
        StepLossCode::AppearanceReducedToBaseColor,
        StepLossCode::HiddenAppearanceVisibilityUnsupported,
    ]) {
        coverage.appearance_bindings = Reported;
    }
    if has(&[
        StepLossCode::LayerItemWithoutCarrier,
        StepLossCode::HiddenPresentationLayerOmitted,
        StepLossCode::HiddenPresentationLayerVisibilityUnsupported,
    ]) {
        coverage.presentation_layers = Reported;
    }
}
