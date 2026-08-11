// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.3dm` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`RhinoLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`RhinoLossCode::note`] builds each [`LossNote`]: category and severity
//! come from the code; only the per-instance message is caller-supplied.
//! [`RhinoLossCode::shared_code`] matches exhaustively.

use cadmpeg_ir::report::{LossKind, LossNote, Severity};

/// A stable, machine-readable identifier for one `.3dm` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`RhinoLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RhinoLossCode {
    /// Container or table scan surfaced a structural diagnostic.
    ContainerScanDiagnostic,
    /// A stored checksum does not match the protected bytes.
    IntegrityFailure,
    /// A framed presentation record could not be transferred.
    PresentationRecordDropped,
    /// Mesh n-gon grouping is not represented in neutral tessellation.
    MeshNgonGroupingDropped,
    /// Embedded history geometry could not be decoded.
    HistoryEmbeddedGeometryDropped,
    /// A dimension-style override object was not applied.
    DimensionOverrideDropped,
    /// A history dependency points to a later producer and cannot enter ordered IR.
    HistoryDependencyDropped,
    /// Instance-definition records were malformed, ambiguous, or checksum-degraded.
    ContainerInstanceDefinitionDegraded,
    /// Census of object records framed against object records transferred.
    ObjectRecordCensus,
    /// An object class is not decoded; its records carry only retained bytes.
    ObjectFamilyNotTransferred,
    /// Object attributes (name, layer, color, visibility) did not transfer whole.
    ObjectAttributesDegraded,
    /// A framed object record could not be decoded from its payload.
    ObjectFramingUndecodable,
    /// A decode phase surfaced a per-record diagnostic.
    ObjectDecodeDiagnostic,
    /// A discontinuous polycurve join moved both source endpoints to their midpoint.
    PolycurveJoinGap,
    /// One B-rep trim lost its parameter-space curve while its topology remained.
    TrimPcurveDropped,
    /// B-rep topology fell back to a carrier-only transfer.
    TopologyBrepFallback,
    /// Hatch fill pattern is retained as a native pattern index, not a filled region.
    HatchFillNotTransferred,
    /// Polyedge segment references are retained without resolved edge identities.
    PolyedgeReferencesNotResolved,
    /// Detail-view projection state is retained as a digest, not a decoded view.
    DetailViewNotTransferred,
    /// Trivariate cage control lattice is retained as text, not a typed deformation.
    CageLatticeNotTransferred,
    /// Space-morph deformation is retained as native parameters, not applied.
    MorphDeformationNotApplied,
    /// Curve-on-surface trim binding is retained as native parameters.
    CurveOnSurfaceBindingNotTransferred,
    /// A dimension style reference does not resolve to a decoded style record.
    DimensionStyleUnresolved,
    /// A dimension detail-view reference does not resolve to a decoded view.
    DimensionDetailReferenceUnresolved,
    /// A definition member or captive object UUID does not resolve to one record.
    ReferenceMemberUnresolved,
    /// A definition member or captive object UUID resolves to several records.
    ReferenceMemberAmbiguous,
    /// History-record geometry is retained without a neutral carrier.
    HistoryGeometryNotTransferred,
    /// Standalone mesh vertices are written at reduced (f32) precision.
    MeshVertexPrecisionReduced,
    /// Mesh normals are written at reduced (f32) precision.
    MeshNormalPrecisionReduced,
}

impl RhinoLossCode {
    /// Every code, in declaration order.
    pub const ALL: &'static [RhinoLossCode] = &[
        Self::ContainerScanDiagnostic,
        Self::IntegrityFailure,
        Self::PresentationRecordDropped,
        Self::MeshNgonGroupingDropped,
        Self::HistoryEmbeddedGeometryDropped,
        Self::DimensionOverrideDropped,
        Self::HistoryDependencyDropped,
        Self::ContainerInstanceDefinitionDegraded,
        Self::ObjectRecordCensus,
        Self::ObjectFamilyNotTransferred,
        Self::ObjectAttributesDegraded,
        Self::ObjectFramingUndecodable,
        Self::ObjectDecodeDiagnostic,
        Self::PolycurveJoinGap,
        Self::TrimPcurveDropped,
        Self::TopologyBrepFallback,
        Self::HatchFillNotTransferred,
        Self::PolyedgeReferencesNotResolved,
        Self::DetailViewNotTransferred,
        Self::CageLatticeNotTransferred,
        Self::MorphDeformationNotApplied,
        Self::CurveOnSurfaceBindingNotTransferred,
        Self::DimensionStyleUnresolved,
        Self::DimensionDetailReferenceUnresolved,
        Self::ReferenceMemberUnresolved,
        Self::ReferenceMemberAmbiguous,
        Self::HistoryGeometryNotTransferred,
        Self::MeshVertexPrecisionReduced,
        Self::MeshNormalPrecisionReduced,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ContainerScanDiagnostic => "container.scan-diagnostic",
            Self::IntegrityFailure => "container.integrity-failure",
            Self::PresentationRecordDropped => "presentation.record-dropped",
            Self::MeshNgonGroupingDropped => "mesh.ngon-grouping-dropped",
            Self::HistoryEmbeddedGeometryDropped => "history.embedded-geometry-dropped",
            Self::DimensionOverrideDropped => "dimension.override-dropped",
            Self::HistoryDependencyDropped => "history.dependency-dropped",
            Self::ContainerInstanceDefinitionDegraded => "container.instance-definition-degraded",
            Self::ObjectRecordCensus => "object.record-census",
            Self::ObjectFamilyNotTransferred => "object.family-not-transferred",
            Self::ObjectAttributesDegraded => "object.attributes-degraded",
            Self::ObjectFramingUndecodable => "object.framing-undecodable",
            Self::ObjectDecodeDiagnostic => "object.decode-diagnostic",
            Self::PolycurveJoinGap => "curve.polycurve-join-gap",
            Self::TrimPcurveDropped => "brep.trim-pcurve-dropped",
            Self::TopologyBrepFallback => "topology.brep-fallback",
            Self::HatchFillNotTransferred => "hatch.fill-not-transferred",
            Self::PolyedgeReferencesNotResolved => "polyedge.references-not-resolved",
            Self::DetailViewNotTransferred => "detail.view-not-transferred",
            Self::CageLatticeNotTransferred => "cage.lattice-not-transferred",
            Self::MorphDeformationNotApplied => "morph.deformation-not-applied",
            Self::CurveOnSurfaceBindingNotTransferred => "curve-on-surface.binding-not-transferred",
            Self::DimensionStyleUnresolved => "dimension.style-unresolved",
            Self::DimensionDetailReferenceUnresolved => "dimension.detail-reference-unresolved",
            Self::ReferenceMemberUnresolved => "reference.member-unresolved",
            Self::ReferenceMemberAmbiguous => "reference.member-ambiguous",
            Self::HistoryGeometryNotTransferred => "history.geometry-not-transferred",
            Self::MeshVertexPrecisionReduced => "mesh.vertex-precision-reduced",
            Self::MeshNormalPrecisionReduced => "mesh.normal-precision-reduced",
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::ObjectRecordCensus => Severity::Info,
            Self::ObjectFramingUndecodable | Self::IntegrityFailure => Severity::Error,
            _ => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_code(self) -> LossKind {
        match self {
            Self::ContainerScanDiagnostic
            | Self::ContainerInstanceDefinitionDegraded
            | Self::ObjectFramingUndecodable
            | Self::ObjectDecodeDiagnostic
            | Self::PolycurveJoinGap
            | Self::ReferenceMemberUnresolved
            | Self::ReferenceMemberAmbiguous => LossKind::DecodeDiagnostic,
            Self::TrimPcurveDropped => LossKind::PcurveOmitted,
            Self::IntegrityFailure => LossKind::IntegrityFailure,
            Self::PresentationRecordDropped => LossKind::AssetNotTransferred,
            Self::MeshNgonGroupingDropped => LossKind::RecordNotTyped,
            Self::HistoryEmbeddedGeometryDropped => LossKind::GeometryNotTransferred,
            Self::DimensionOverrideDropped => LossKind::PmiOmitted,
            Self::HistoryDependencyDropped => LossKind::ReferenceGraphNotClosed,
            Self::ObjectRecordCensus => LossKind::ObjectRecordsUntransferred,
            Self::ObjectFamilyNotTransferred => LossKind::UnsupportedObjectFamily,
            Self::ObjectAttributesDegraded => LossKind::AttributesNotTransferred,
            Self::TopologyBrepFallback => LossKind::TopologyNotTransferred,
            Self::HatchFillNotTransferred
            | Self::PolyedgeReferencesNotResolved
            | Self::DetailViewNotTransferred
            | Self::CageLatticeNotTransferred
            | Self::MorphDeformationNotApplied
            | Self::CurveOnSurfaceBindingNotTransferred
            | Self::HistoryGeometryNotTransferred => LossKind::RecordNotTyped,
            Self::DimensionStyleUnresolved | Self::DimensionDetailReferenceUnresolved => {
                LossKind::PmiOmitted
            }
            Self::MeshVertexPrecisionReduced | Self::MeshNormalPrecisionReduced => {
                LossKind::MeshVertexPrecision
            }
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// Message form is `"<code>: <message>"`. Severity comes from the code;
    /// provenance is absent unless the caller adds it with
    /// [`LossNote::with_provenance`].
    #[must_use]
    pub fn note(self, message: impl std::fmt::Display) -> LossNote {
        LossNote::new(self.shared_code(), format!("{}: {message}", self.code()))
            .with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::RhinoLossCode;
    use std::collections::BTreeSet;

    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = RhinoLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "container.scan-diagnostic",
                "container.integrity-failure",
                "presentation.record-dropped",
                "mesh.ngon-grouping-dropped",
                "history.embedded-geometry-dropped",
                "dimension.override-dropped",
                "history.dependency-dropped",
                "container.instance-definition-degraded",
                "object.record-census",
                "object.family-not-transferred",
                "object.attributes-degraded",
                "object.framing-undecodable",
                "object.decode-diagnostic",
                "curve.polycurve-join-gap",
                "brep.trim-pcurve-dropped",
                "topology.brep-fallback",
                "hatch.fill-not-transferred",
                "polyedge.references-not-resolved",
                "detail.view-not-transferred",
                "cage.lattice-not-transferred",
                "morph.deformation-not-applied",
                "curve-on-surface.binding-not-transferred",
                "dimension.style-unresolved",
                "dimension.detail-reference-unresolved",
                "reference.member-unresolved",
                "reference.member-ambiguous",
                "history.geometry-not-transferred",
                "mesh.vertex-precision-reduced",
                "mesh.normal-precision-reduced",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in RhinoLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    /// The note builder fixes severity from the code and emits the code string.
    #[test]
    fn note_takes_severity_from_the_code_and_renders_it() {
        for code in RhinoLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, format!("{}: x", code.code()));
            assert!(note.provenance.is_none());
        }
    }
}
