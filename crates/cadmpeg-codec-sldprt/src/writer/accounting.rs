// SPDX-License-Identifier: Apache-2.0
//! Count records omitted inside arenas written by the semantic exporter.

use crate::loss::SldprtLossCode;
use cadmpeg_ir::{
    appearance::AppearanceTarget,
    geometry::{CurveGeometry, SolvedCurveGeometry},
    report::loss::LossNote,
    CadIr,
};

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
            .filter(|attribute| !attribute.id.as_str().starts_with("sldprt:"))
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
    losses
}
