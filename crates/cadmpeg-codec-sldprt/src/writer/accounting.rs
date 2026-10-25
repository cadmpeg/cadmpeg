// SPDX-License-Identifier: Apache-2.0
//! Count records omitted inside arenas written by the semantic exporter.

use crate::loss::SldprtLossCode;
use cadmpeg_ir::{
    appearance::AppearanceTarget,
    geometry::{CurveGeometry, SolvedCurveGeometry},
    report::loss::LossNote,
    CadIr,
};

pub(super) fn unsupported_attribute(attribute: &cadmpeg_ir::attributes::SourceAttribute) -> bool {
    attribute.values.iter().any(|value| {
        matches!(
            value,
            cadmpeg_ir::attributes::AttributeValue::Map(_)
                | cadmpeg_ir::attributes::AttributeValue::Expression(_)
                | cadmpeg_ir::attributes::AttributeValue::SubReference { .. }
                | cadmpeg_ir::attributes::AttributeValue::ExternalReference { .. }
                | cadmpeg_ir::attributes::AttributeValue::NumericList(_)
        )
    })
}

pub(super) fn losses(ir: &CadIr, regenerated_brep: bool) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let mut charge = |count: usize, code: SldprtLossCode, records: &str| {
        if count != 0 {
            losses.push(code.note(format!(
                "{count} {records} record(s) were not written by the SLDPRT semantic exporter"
            )));
        }
    };
    if regenerated_brep {
        let edges = ir
            .model
            .shells
            .iter()
            .flat_map(cadmpeg_ir::topology::Shell::wire_edges)
            .collect::<std::collections::BTreeSet<_>>();
        let vertices = ir
            .model
            .shells
            .iter()
            .flat_map(cadmpeg_ir::topology::Shell::free_vertices)
            .collect::<std::collections::BTreeSet<_>>();
        charge(
            edges.len(),
            SldprtLossCode::WriterShellWireTopologyOmitted,
            "shell wire edge ownership",
        );
        charge(
            vertices.len(),
            SldprtLossCode::WriterShellWireTopologyOmitted,
            "shell free vertex ownership",
        );
        charge(
            ir.model
                .coedges
                .iter()
                .map(|coedge| coedge.pcurves.len())
                .sum(),
            SldprtLossCode::WriterCoedgePcurveUsesOmitted,
            "coedge pcurve use",
        );
        charge(
            ir.model
                .curves
                .iter()
                .filter(|curve| {
                    matches!(
                        curve.geometry,
                        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_))
                    ) && curve
                        .id
                        .as_str()
                        .starts_with("sldprt:brep:curve#sphere-seam-face:")
                })
                .count(),
            SldprtLossCode::WriterSphereSeamCurvesOmitted,
            "degenerate sphere-seam curve",
        );
    }
    charge(
        ir.model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                !matches!(
                    binding.target,
                    AppearanceTarget::Face(_) | AppearanceTarget::Body(_)
                )
            })
            .count(),
        SldprtLossCode::WriterAppearanceBindingsOmitted,
        "appearance binding to a target other than a face or body",
    );
    charge(
        ir.model
            .attributes
            .iter()
            .filter(|attribute| {
                !(attribute.id.as_str().starts_with("sldprt:")
                    || super::custom_properties::represents(attribute)
                    || (matches!(
                        attribute.target,
                        cadmpeg_ir::attributes::AttributeTarget::Pmi(_)
                    ) && matches!(
                        attribute.name.as_str(),
                        "precision" | "basic" | "inspection" | "reference_only"
                    )))
            })
            .count(),
        SldprtLossCode::WriterAttributesOmitted,
        "attribute outside the sldprt namespace",
    );
    let bound = ir
        .model
        .appearance_bindings
        .iter()
        .filter(|binding| {
            matches!(
                binding.target,
                AppearanceTarget::Face(_) | AppearanceTarget::Body(_)
            )
        })
        .map(|binding| &binding.appearance)
        .collect::<std::collections::BTreeSet<_>>();
    charge(
        ir.model
            .appearances
            .iter()
            .filter(|appearance| {
                appearance.schema.as_deref() != Some("moVisualProperties_c")
                    && !bound.contains(&appearance.id)
            })
            .count(),
        SldprtLossCode::WriterUnboundAppearancesOmitted,
        "unbound appearance outside moVisualProperties_c",
    );
    charge(
        ir.model
            .tessellations
            .iter()
            .filter(|mesh| mesh.body.is_some())
            .count(),
        SldprtLossCode::WriterTessellationMetadataOmitted,
        "tessellation body binding",
    );
    charge(
        ir.model
            .tessellations
            .iter()
            .map(|mesh| mesh.faces.len())
            .sum(),
        SldprtLossCode::WriterTessellationMetadataOmitted,
        "tessellation face binding",
    );
    charge(
        ir.model
            .tessellations
            .iter()
            .filter(|mesh| mesh.chordal_deflection().is_some())
            .count(),
        SldprtLossCode::WriterTessellationMetadataOmitted,
        "tessellation chordal deflection",
    );
    for attribute in ir
        .model
        .attributes
        .iter()
        .filter(|attribute| unsupported_attribute(attribute))
    {
        losses.push(SldprtLossCode::WriterAttributesOmitted.note(format!(
            "attribute {} ({}) has no representable SLDPRT attribute value",
            attribute.id, attribute.name
        )));
    }
    losses
}

/// Charge whole arenas the SLDPRT format writer cannot encode.
pub(super) fn arena_losses(
    ir: &CadIr,
    coverage: &cadmpeg_ir::codec::write::ArenaDispositions,
) -> Vec<LossNote> {
    use cadmpeg_ir::codec::write::ArenaDisposition;
    let mut losses = Vec::new();
    macro_rules! charge {
        ($($arena:ident),* $(,)?) => {$(
            if coverage.$arena == ArenaDisposition::Dropped && !ir.model.$arena.is_empty() {
                losses.push(SldprtLossCode::WriterArenaOmitted.note(format!(
                    "model.{}: {} record(s) omitted by the SLDPRT writer", stringify!($arena), ir.model.$arena.len()
                )));
            }
        )*};
    }
    charge!(
        subds,
        assets,
        feature_input_topologies,
        feature_result_topologies,
        spatial_sketch_constraints,
        spreadsheets,
        product_definitions,
        occurrences,
        assembly_joints,
        drawings,
        semantic_annotations,
        presentation_documents,
        view_presentations,
        presentation_layers,
        pmi,
        pcurves,
        procedural_curves,
        procedural_surfaces,
        configurations
    );
    losses
}
