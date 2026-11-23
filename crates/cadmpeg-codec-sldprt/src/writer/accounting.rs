// SPDX-License-Identifier: Apache-2.0
//! Count records omitted inside arenas written by the semantic exporter.

use std::collections::BTreeMap;

use crate::loss::{NamedCount, SldprtLossCode};
use cadmpeg_ir::{
    appearance::AppearanceTarget,
    geometry::{CurveGeometry, SolvedCurveGeometry},
    report::loss::LossNote,
    schema::EntityKind,
    CadIr,
};

pub(super) fn losses(ir: &CadIr, regenerated_brep: bool) -> Vec<LossNote> {
    let unwritten_bindings = if regenerated_brep {
        super::face_colors_and_unwritten_bindings(ir).1
    } else {
        Vec::new()
    };
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
    charge(
        crate::resolved_features::write_prepare::omitted_driven_dimensions(ir),
        SldprtLossCode::WriterDrivenDimensionsOmitted,
        "driven (reference) sketch dimension",
    );
    let omitted = crate::resolved_features::write_prepare::omitted_driven_parameters(ir);
    charge(
        ir.model
            .parameters
            .iter()
            .filter(|parameter| omitted.contains(&parameter.id))
            .count(),
        SldprtLossCode::WriterDrivenDimensionsOmitted,
        "driven (reference) dimension parameter",
    );
    // A write that reduces a foreign history writes every expression with its
    // value, or leaves out one with no value, and names it there
    // (`history_drop`).
    let readers = (!super::history_drop::HistoryDrop::applies(ir)).then(|| {
        crate::history::write::parameters::omitted_readers(&ir.model.parameters, &omitted)
    });
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
    charge(
        ir.model
            .tessellations
            .iter()
            .map(|mesh| mesh.triangle_groups().len())
            .sum(),
        SldprtLossCode::WriterTessellationMetadataOmitted,
        "tessellation triangle group",
    );
    charge(
        ir.model
            .tessellations
            .iter()
            .map(|mesh| mesh.texture_assignments().len())
            .sum(),
        SldprtLossCode::WriterTessellationMetadataOmitted,
        "tessellation texture assignment",
    );
    charge(
        ir.model
            .tessellations
            .iter()
            .map(|mesh| mesh.texture_coordinates().len())
            .sum(),
        SldprtLossCode::WriterTessellationMetadataOmitted,
        "tessellation texture coordinate set",
    );
    charge(
        ir.model
            .tessellations
            .iter()
            .map(|mesh| mesh.feature_edges().len())
            .sum(),
        SldprtLossCode::WriterTessellationMetadataOmitted,
        "tessellation feature edge",
    );
    if !unwritten_bindings.is_empty() {
        losses.push(
            SldprtLossCode::WriterAppearanceBindingsOmitted.note(format!(
                "{} face appearance binding(s) state no colour, or a colour other than the one \
             the face already has; the face is written with that one: {}",
                unwritten_bindings.len(),
                unwritten_bindings.join(", ")
            )),
        );
    }
    if let Some(readers) = readers {
        let ids = |readers: &[&cadmpeg_ir::features::parameters::DesignParameter]| {
            readers
                .iter()
                .map(|parameter| parameter.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        if !readers.valued.is_empty() {
            losses.push(
                SldprtLossCode::WriterParameterExpressionsOmitted.note(format!(
                    "the SLDPRT writer writes {} parameter(s) with their value in place of an \
                 expression that reads an omitted driven (reference) dimension parameter: {}",
                    readers.valued.len(),
                    ids(&readers.valued)
                )),
            );
        }
        if !readers.dropped.is_empty() {
            losses.push(SldprtLossCode::WriterFeaturesOmitted.note(format!(
                "the SLDPRT writer leaves out {} parameter(s) whose expression reads an omitted \
                 parameter and that hold no value to write in its place: {}",
                readers.dropped.len(),
                ids(&readers.dropped)
            )));
        }
    }
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
    /// An integer is wider than its record's 32-bit field (§3.4,
    /// `export:value.unwritable`).
    TooWide,
    /// The exporter cannot add, regenerate or edit this record in place.
    NoSerializer,
    /// The attribute's retained source position does not hold its record.
    RetainedMismatch,
    /// A saved property the exporter cannot serialize; its carrier is
    /// written as stored, or no carrier row is written.
    SavedProperty,
    /// A retained record the document no longer states; its bytes are kept.
    Removed,
    /// The record's retained member is written as stored: the source holds
    /// it in more than one member, unnamed, or with bytes its envelope
    /// member does not hold.
    OwnerCarried,
    /// The source fidelity holds no bytes of the record's member.
    OwnerUnavailable,
    /// The record's member is generated from other records, which replace
    /// the patched member.
    OwnerRegenerated,
}

/// What the output holds for one attribute the semantic exporter did not
/// write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Held {
    /// The record as the source file stores it: the attribute's edit is
    /// dropped (`export:edit.dropped`).
    Source,
    /// No record: the attribute was added, or its record is generated
    /// (`writer.attributes-omitted`).
    Nothing,
}

impl Unwritten {
    /// What the attributes state.
    fn stated(self) -> &'static str {
        match self {
            Self::NoValue => "state a null value",
            Self::EntityTarget => {
                "are attached to a part element, but SLDPRT metadata records belong to the document"
            }
            Self::UnknownName => "name no SLDPRT metadata record",
            Self::Unrepresentable => {
                "hold a value whose type, count or range their SLDPRT record cannot hold"
            }
            Self::TooWide => "hold an integer wider than their SLDPRT record's 32-bit word",
            Self::NoSerializer => {
                "state a record the SLDPRT semantic exporter cannot add, regenerate or edit in place"
            }
            Self::RetainedMismatch => {
                "name a retained source position that does not hold their SLDPRT record"
            }
            Self::SavedProperty => {
                "are saved properties the SLDPRT semantic exporter cannot serialize"
            }
            Self::Removed => "are no longer stated by the document",
            Self::OwnerCarried => {
                "are stored in a retained member the SLDPRT semantic exporter does not patch: \
                 one of several, unnamed, or unlike its source envelope member"
            }
            Self::OwnerUnavailable => {
                "are stored in a member whose bytes the source fidelity does not hold"
            }
            Self::OwnerRegenerated => {
                "are stored in a member the SLDPRT semantic exporter generates from other records"
            }
        }
    }

    /// What the exporter wrote instead.
    fn instead(self, held: Held) -> &'static str {
        match (self, held) {
            (Self::SavedProperty, Held::Source) => {
                "so it wrote the property carrier as the source stored it"
            }
            (Self::SavedProperty, Held::Nothing) => "so it wrote no property carrier row for them",
            (Self::Removed, _) => {
                "but the SLDPRT semantic exporter kept their retained source bytes"
            }
            (_, Held::Source) => {
                "so the SLDPRT semantic exporter kept each one's retained source bytes"
            }
            (_, Held::Nothing) => "so the SLDPRT semantic exporter wrote no record",
        }
    }
}

/// The metadata attributes one semantic write did not write, by id. The
/// first reason recorded for an id is kept.
#[derive(Clone, Debug, Default)]
pub(crate) struct UnwrittenAttributes(BTreeMap<String, (Unwritten, Held)>);

impl UnwrittenAttributes {
    pub(super) fn record(&mut self, id: &str, reason: Unwritten, held: Held) {
        self.0.entry(id.to_owned()).or_insert((reason, held));
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    #[cfg(test)]
    pub(crate) fn reason(&self, id: &str) -> Option<Unwritten> {
        self.0.get(id).map(|(reason, _)| *reason)
    }

    /// One note per reason and holding: `export:edit.dropped` naming every
    /// record kept as the source stored it, and `WriterAttributesOmitted`
    /// naming the attributes with no record.
    pub(crate) fn notes(&self) -> Vec<LossNote> {
        let mut grouped = BTreeMap::<(Held, Unwritten), Vec<&str>>::new();
        for (id, (reason, held)) in &self.0 {
            grouped.entry((*held, *reason)).or_default().push(id);
        }
        grouped
            .into_iter()
            .filter_map(|((held, reason), ids)| match (reason, held) {
                (Unwritten::TooWide, held) => cadmpeg_ir::codec::write::value_unwritable(
                    "SLDPRT",
                    &format!(
                        "names sldprt metadata attribute records that {}, {}",
                        reason.stated(),
                        reason.instead(held)
                    ),
                    ids,
                ),
                (_, Held::Source) => cadmpeg_ir::codec::write::edit_dropped(
                    EntityKind::SourceAttribute,
                    "SLDPRT",
                    &format!(
                        "names sldprt metadata attribute records that {}, {}",
                        reason.stated(),
                        reason.instead(held)
                    ),
                    ids,
                ),
                (_, Held::Nothing) => {
                    let head = format!(
                        "{} sldprt metadata attribute record(s) {}, {}",
                        ids.len(),
                        reason.stated(),
                        reason.instead(held)
                    );
                    let mut named = NamedCount::default();
                    for id in ids {
                        named.push(id);
                    }
                    Some(SldprtLossCode::WriterAttributesOmitted.note(format!("{head}: {named}")))
                }
            })
            .collect()
    }
}
