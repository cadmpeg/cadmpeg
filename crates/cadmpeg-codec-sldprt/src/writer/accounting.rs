// SPDX-License-Identifier: Apache-2.0
//! Count records omitted inside arenas written by the semantic exporter.

use std::collections::BTreeMap;

use crate::loss::{NamedCount, SldprtLossCode};
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
            .geometry
            .shells
            .iter()
            .flat_map(cadmpeg_ir::topology::Shell::wire_edges)
            .collect::<std::collections::BTreeSet<_>>();
        let vertices = ir
            .geometry
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
            ir.geometry
                .coedges
                .iter()
                .map(|coedge| coedge.pcurves.len())
                .sum(),
            SldprtLossCode::WriterCoedgePcurveUsesOmitted,
            "coedge pcurve use",
        );
        charge(
            ir.geometry
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
                    binding.target.entity,
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
                binding.target.entity,
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

/// Why the semantic exporter did not write one metadata attribute.
///
/// The order is the order of the loss notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Unwritten {
    /// A position states no value.
    NoValue,
    /// The attribute is attached to a part element; SLDPRT metadata records
    /// belong to the document.
    EntityTarget,
    /// No SLDPRT metadata record has the attribute's name.
    UnknownName,
    /// A value has a type, count or range its record cannot hold.
    Unrepresentable,
    /// The exporter cannot add, regenerate or edit this record in place.
    NoSerializer,
    /// The attribute's retained source position does not hold its record.
    RetainedMismatch,
    /// A saved-property edit; the property carrier is written as stored.
    SavedProperty,
    /// A retained record the document no longer states; its bytes are kept.
    Removed,
}

impl Unwritten {
    /// What the attributes state, and whether the shared tail on what the
    /// exporter wrote instead follows.
    fn phrase(self) -> (&'static str, bool) {
        match self {
            Self::NoValue => ("state a null value", true),
            Self::EntityTarget => (
                "are attached to a part element, but SLDPRT metadata records belong to the document",
                true,
            ),
            Self::UnknownName => ("name no SLDPRT metadata record", true),
            Self::Unrepresentable => (
                "hold a value whose type, count or range their SLDPRT record cannot hold",
                true,
            ),
            Self::NoSerializer => (
                "state a record the SLDPRT semantic exporter cannot add, regenerate or edit in place",
                true,
            ),
            Self::RetainedMismatch => (
                "name a retained source position that does not hold their SLDPRT record",
                true,
            ),
            Self::SavedProperty => (
                "are saved-property edits the SLDPRT semantic exporter cannot serialize, so it wrote the property carrier as the source stored it, or none where the source had none",
                false,
            ),
            Self::Removed => (
                "are no longer stated by the document, but the SLDPRT semantic exporter kept their retained source bytes",
                false,
            ),
        }
    }
}

/// The metadata attributes one semantic write did not write, by id. The
/// first reason recorded for an id is kept.
#[derive(Clone, Debug, Default)]
pub(crate) struct UnwrittenAttributes(BTreeMap<String, Unwritten>);

impl UnwrittenAttributes {
    pub(super) fn record(&mut self, id: &str, reason: Unwritten) {
        self.0.entry(id.to_owned()).or_insert(reason);
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    #[cfg(test)]
    pub(crate) fn reason(&self, id: &str) -> Option<Unwritten> {
        self.0.get(id).copied()
    }

    /// One `WriterAttributesOmitted` note per reason, naming its ids.
    pub(crate) fn notes(&self) -> Vec<LossNote> {
        let mut grouped = BTreeMap::<Unwritten, NamedCount>::new();
        for (id, reason) in &self.0 {
            grouped.entry(*reason).or_default().push(id);
        }
        grouped
            .into_iter()
            .map(|(reason, ids)| {
                let (stated, kept) = reason.phrase();
                let kept = if kept {
                    ", so the SLDPRT semantic exporter kept each one's retained source bytes or wrote no record"
                } else {
                    ""
                };
                SldprtLossCode::WriterAttributesOmitted.note(format!(
                    "{} sldprt metadata attribute record(s) {stated}{kept}: {ids}",
                    ids.count()
                ))
            })
            .collect()
    }
}
