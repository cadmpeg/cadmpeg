// SPDX-License-Identifier: Apache-2.0
//! STEP target resolution and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, ArenaDisposition, ArenaDispositions, Consumption,
    EncodeInput, ExportBody, NativeDispositions, WritePath,
};

use crate::export::write_step_outcome;
use crate::loss::StepLossCode;
use crate::options::StepSchema;
use crate::StepCodec;

/// Resolve the request against the source and synthesize the selected schema.
///
/// STEP has no retained-image path, so a source schema outside the catalog
/// is written as the catalog default (§3.4, Refusals, `Inherit`).
pub(crate) fn plan(
    codec: &StepCodec,
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let schema = resolved
        .synthesized_row()
        .and_then(|(index, _)| StepSchema::from_target_index(index));
    debug_assert!(
        schema.is_some(),
        "the STEP catalog declares a default and is projected from StepSchema::ALL"
    );
    let schema = schema.unwrap_or(StepSchema::DEFAULT);
    let mut bytes = Vec::new();
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(input.ir)?;
    let outcome = write_step_outcome(
        &recovered.document,
        &mut bytes,
        schema,
        &codec.options,
        recovered.recovered_edges,
    )?;
    let mut losses = outcome.losses;
    if !recovered.sampled_agreements.is_empty() {
        losses.push(StepLossCode::EdgePcurveAgreementSampled.note(format!(
            "{} recovered edge(s) have coedge curves compared at five parameters; agreement between samples is not established",
            recovered.sampled_agreements.len()
        )));
    }
    if !recovered.sampled_fits.is_empty() {
        losses.push(StepLossCode::EdgePcurveSampledToBspline.note(format!(
            "{} recovered edge(s) were fitted as B-splines with sampled residual at most {} model length units",
            recovered.sampled_fits.len(), recovered.residual_limit
        )));
    }
    Ok(ExportBody {
        bytes,
        census: outcome.census,
        write_path: WritePath::Synthesized {
            // STEP has no retained image: a provided fidelity is never replayed.
            consumption: Consumption::NotConsumed,
        },
        coverage: ArenaCoverage::Declared(coverage(schema)),
        // STEP holds no native record of any format.
        native: NativeDispositions::dropped("which the STEP writer does not write"),
        losses,
        notes: outcome.notes,
        written_row: None,
        annotations: None,
    })
}

/// What the STEP writer does with each model arena.
///
/// `Builder::note_unrepresented` in `export.rs` charges a specific loss for
/// every arena or record the graph does not carry. Arenas charged as a whole
/// are `Dropped`.
fn coverage(schema: StepSchema) -> ArenaDispositions {
    use ArenaDisposition::{Dropped, Ignored, Omitted, Reported, Written};
    let ap242 = |supported: bool| if supported { Written } else { Reported };
    ArenaDispositions {
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
        point_clouds: Omitted,
        history_bodies: Ignored,
        appearances: Written,
        appearance_bindings: Written,
        attributes: Dropped,
        pmi: ap242(schema.supports_semantic_pmi()),
        presentation_layers: Written,
        physical_materials: Omitted,
        mass_properties: Omitted,
        rigid_bodies: Omitted,
        colliders: Omitted,
        articulations: Omitted,
        physics_scenes: Omitted,
        solver_settings: Omitted,
        declared_measures: Omitted,
        design: cadmpeg_ir::codec::write::DesignArenaDispositions::uniform(Omitted),
        process: cadmpeg_ir::codec::write::ProcessArenaDispositions::uniform(Omitted),
    }
}
