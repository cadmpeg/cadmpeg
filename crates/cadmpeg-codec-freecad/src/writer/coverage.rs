// SPDX-License-Identifier: Apache-2.0
//! Explicit neutral arena and loss accounting for synthesized `FCStd`.

use crate::loss::FreecadLossCode;
use cadmpeg_ir::{
    codec::write::{ArenaDisposition, ArenaDispositions},
    document::CadIr,
    report::loss::LossNote,
};

pub(super) fn declaration() -> ArenaDispositions {
    ArenaDispositions {
        bodies: ArenaDisposition::Dropped,
        regions: ArenaDisposition::Dropped,
        shells: ArenaDisposition::Dropped,
        faces: ArenaDisposition::Dropped,
        loops: ArenaDisposition::Dropped,
        coedges: ArenaDisposition::Dropped,
        edges: ArenaDisposition::Dropped,
        vertices: ArenaDisposition::Dropped,
        points: ArenaDisposition::Dropped,
        surfaces: ArenaDisposition::Dropped,
        curves: ArenaDisposition::Dropped,
        subds: ArenaDisposition::Dropped,
        pcurves: ArenaDisposition::Dropped,
        procedural_surfaces: ArenaDisposition::Dropped,
        procedural_curves: ArenaDisposition::Dropped,
        assets: ArenaDisposition::Dropped,
        features: ArenaDisposition::Dropped,
        feature_input_topologies: ArenaDisposition::Dropped,
        feature_result_topologies: ArenaDisposition::Dropped,
        configurations: ArenaDisposition::Dropped,
        parameters: ArenaDisposition::Dropped,
        sketches: ArenaDisposition::Dropped,
        sketch_entities: ArenaDisposition::Dropped,
        sketch_constraints: ArenaDisposition::Dropped,
        spatial_sketches: ArenaDisposition::Dropped,
        spatial_sketch_entities: ArenaDisposition::Dropped,
        spatial_sketch_constraints: ArenaDisposition::Dropped,
        spreadsheets: ArenaDisposition::Dropped,
        product_definitions: ArenaDisposition::Dropped,
        occurrences: ArenaDisposition::Dropped,
        assembly_joints: ArenaDisposition::Dropped,
        drawings: ArenaDisposition::Dropped,
        semantic_annotations: ArenaDisposition::Dropped,
        presentation_documents: ArenaDisposition::Dropped,
        view_presentations: ArenaDisposition::Dropped,
        tessellations: ArenaDisposition::Dropped,
        appearances: ArenaDisposition::Dropped,
        appearance_bindings: ArenaDisposition::Dropped,
        attributes: ArenaDisposition::Dropped,
        pmi: ArenaDisposition::Dropped,
        presentation_layers: ArenaDisposition::Dropped,
    }
}

/// XML scalar reductions affect metadata rather than solved geometry. These
/// families can share one native object/property, so report their coverage
/// conservatively whenever that output document contains such a reduction.
pub(super) fn xml_reductions(coverage: &mut ArenaDispositions, document: bool, gui: bool) {
    if document {
        coverage.bodies = ArenaDisposition::Reported;
        coverage.features = ArenaDisposition::Reported;
        coverage.parameters = ArenaDisposition::Reported;
        coverage.sketches = ArenaDisposition::Reported;
        coverage.sketch_entities = ArenaDisposition::Reported;
        coverage.sketch_constraints = ArenaDisposition::Reported;
        coverage.spreadsheets = ArenaDisposition::Reported;
        coverage.product_definitions = ArenaDisposition::Reported;
        coverage.occurrences = ArenaDisposition::Reported;
        coverage.assembly_joints = ArenaDisposition::Reported;
    }
    if gui {
        coverage.presentation_documents = ArenaDisposition::Reported;
        coverage.view_presentations = ArenaDisposition::Reported;
        coverage.appearances = ArenaDisposition::Reported;
        coverage.appearance_bindings = ArenaDisposition::Reported;
    }
}

pub(super) fn complete(ir: &CadIr, losses: &mut Vec<LossNote>) -> ArenaDispositions {
    let mut dispositions = declaration();
    dispositions.bodies = ArenaDisposition::Written;
    dispositions.regions = ArenaDisposition::Written;
    dispositions.shells = ArenaDisposition::Written;
    dispositions.faces = ArenaDisposition::Written;
    dispositions.loops = ArenaDisposition::Written;
    dispositions.coedges = ArenaDisposition::Written;
    dispositions.edges = ArenaDisposition::Written;
    dispositions.vertices = ArenaDisposition::Written;
    dispositions.points = ArenaDisposition::Written;
    dispositions.curves = ArenaDisposition::Written;
    dispositions.surfaces = ArenaDisposition::Written;
    dispositions.pcurves = ArenaDisposition::Written;
    dispositions.procedural_curves = ArenaDisposition::Written;
    dispositions.procedural_surfaces = ArenaDisposition::Written;
    dispositions.features = ArenaDisposition::Written;
    dispositions.parameters = ArenaDisposition::Written;
    dispositions.sketches = ArenaDisposition::Written;
    dispositions.sketch_entities = ArenaDisposition::Written;
    dispositions.sketch_constraints = ArenaDisposition::Written;
    dispositions.spreadsheets = ArenaDisposition::Written;
    dispositions.product_definitions = ArenaDisposition::Written;
    dispositions.occurrences = ArenaDisposition::Written;
    dispositions.assembly_joints = ArenaDisposition::Written;
    dispositions.presentation_documents = ArenaDisposition::Written;
    dispositions.view_presentations = ArenaDisposition::Written;
    dispositions.appearances = ArenaDisposition::Written;
    dispositions.appearance_bindings = ArenaDisposition::Written;
    dispositions.attributes = ArenaDisposition::Written;
    if losses
        .iter()
        .any(|loss| loss.code.local_code() == "attribute.not-transferred")
    {
        dispositions.attributes = ArenaDisposition::Reported;
    }

    if losses
        .iter()
        .any(|loss| loss.code.local_code() == "appearance.not-transferred")
    {
        dispositions.appearances = ArenaDisposition::Reported;
        dispositions.appearance_bindings = ArenaDisposition::Reported;
    }
    if losses
        .iter()
        .any(|loss| loss.code.local_code() == "presentation.not-transferred")
    {
        dispositions.presentation_documents = ArenaDisposition::Reported;
        dispositions.view_presentations = ArenaDisposition::Reported;
    }
    if losses
        .iter()
        .any(|loss| loss.code.local_code() == "geometry.procedural-reduced")
    {
        dispositions.procedural_curves = ArenaDisposition::Reported;
        dispositions.procedural_surfaces = ArenaDisposition::Reported;
        dispositions.curves = ArenaDisposition::Reported;
        dispositions.surfaces = ArenaDisposition::Reported;
    }
    let geometry_loss = losses
        .iter()
        .any(|loss| loss.code.local_code() == "geometry.not-transferred");
    let design_loss = losses
        .iter()
        .any(|loss| loss.code.local_code() == "feature.not-transferred");
    if geometry_loss {
        dispositions.bodies = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.regions = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.shells = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.faces = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.loops = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.coedges = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.edges = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.vertices = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.points = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.curves = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.surfaces = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.pcurves = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.procedural_curves = ArenaDisposition::Reported;
    }
    if geometry_loss {
        dispositions.procedural_surfaces = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.features = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.parameters = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.sketches = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.sketch_entities = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.sketch_constraints = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.spreadsheets = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.product_definitions = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.occurrences = ArenaDisposition::Reported;
    }
    if design_loss {
        dispositions.assembly_joints = ArenaDisposition::Reported;
    }

    for (arena, count) in [
        ("subds", ir.model.subds.len()),
        ("assets", ir.model.assets.len()),
        (
            "feature_input_topologies",
            ir.model.feature_input_topologies.len(),
        ),
        (
            "feature_result_topologies",
            ir.model.feature_result_topologies.len(),
        ),
        ("configurations", ir.model.configurations.len()),
        ("spatial_sketches", ir.model.spatial_sketches.len()),
        (
            "spatial_sketch_entities",
            ir.model.spatial_sketch_entities.len(),
        ),
        (
            "spatial_sketch_constraints",
            ir.model.spatial_sketch_constraints.len(),
        ),
        ("drawings", ir.model.drawings.len()),
        ("semantic_annotations", ir.model.semantic_annotations.len()),
        ("tessellations", ir.model.tessellations.len()),
        ("pmi", ir.model.pmi.len()),
        ("presentation_layers", ir.model.presentation_layers.len()),
    ] {
        if count != 0 {
            let code = if matches!(arena, "subds" | "tessellations") {
                FreecadLossCode::ExportGeometryNotTransferred
            } else {
                FreecadLossCode::ExportArenaNotTransferred
            };
            losses.push(code.note(format!(
                "FCStd does not represent {count} record(s) in neutral arena `{arena}`"
            )));
        }
    }
    dispositions
}
