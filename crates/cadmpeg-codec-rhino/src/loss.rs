// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.3dm` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`RhinoLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`RhinoLossCode::note`] is the single construction path for a
//! [`LossNote`] in this crate: it fixes the shared loss category and the
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `rhino` namespace.
//!
//! [`RhinoLossCode::shared_code`] is an exhaustive match with no fall-through
//! arm. A default arm would silently assign a category to a code added later,
//! and the categories this codec spans (geometry, annotation, attribute,
//! diagnostic) have no honest common default.

use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};

/// The phrase every missing-writer-stamp diagnostic carries.
///
/// Parsers deep in the crate report a substituted reading as a warning string;
/// decode promotes any warning containing this phrase to
/// [`RhinoLossCode::SourceWriterStampUnverified`] instead of the generic
/// container/decode diagnostic, the same way integrity and redundant-field
/// diagnostics are promoted.
pub(crate) const WRITER_STAMP_UNVERIFIED_MARKER: &str = "the archive has no writer-version stamp";

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
    /// A recognized annotation userdata payload could not be typed.
    AnnotationUserdataDropped,
    /// Viewport userdata has no typed CADIR owner.
    ViewportUserdataDropped,
    /// Mesh n-gon grouping is not represented in neutral tessellation.
    MeshNgonGroupingDropped,
    /// A stored enumeration value was retained but could not select a neutral value.
    EnumerationValueDegraded,
    /// A redundant count or size was inconsistent; dependent data was dropped.
    RedundantFieldRepaired,
    /// A Brep display-mesh cache slot was wrong-class or unreadable.
    BrepMeshCacheDegraded,
    /// A duplicate source record was resolved by the format's ownership rule.
    DuplicateRecordResolved,
    /// A stored quad was converted to neutral triangles.
    MeshQuadTopologyTriangulated,
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
    /// A field was read under the legacy reading because the archive carries no
    /// openNURBS writer-version stamp to verify that record against.
    ///
    /// Per record, inside a dialect the archive word already identified and
    /// whose grammar this codec verified. It is deliberately *not*
    /// `source.dialect-unverified`: the other codecs pin that string for the
    /// document-level statement "the dialect itself was not verified", which
    /// holds exactly when `Admission::AdmittedUnverified` is reported. Rhino
    /// charges this one inside `Admission::Admitted` documents, so a consumer
    /// joining loss code to admission state must be able to tell them apart.
    /// The taxonomy family below is still the right one.
    SourceWriterStampUnverified,
    /// Body kind came from the closed-shell gauge or from an unverified stored
    /// solid flag rather than from a flag the writer stamp vouches for.
    TopologyBodyKindGaugeSubstituted,
}

impl RhinoLossCode {
    /// Every code, in declaration order.
    pub const ALL: &'static [RhinoLossCode] = &[
        Self::ContainerScanDiagnostic,
        Self::IntegrityFailure,
        Self::PresentationRecordDropped,
        Self::AnnotationUserdataDropped,
        Self::ViewportUserdataDropped,
        Self::MeshNgonGroupingDropped,
        Self::EnumerationValueDegraded,
        Self::RedundantFieldRepaired,
        Self::BrepMeshCacheDegraded,
        Self::DuplicateRecordResolved,
        Self::MeshQuadTopologyTriangulated,
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
        Self::SourceWriterStampUnverified,
        Self::TopologyBodyKindGaugeSubstituted,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ContainerScanDiagnostic => "container.scan-diagnostic",
            Self::IntegrityFailure => "container.integrity-failure",
            Self::PresentationRecordDropped => "presentation.record-dropped",
            Self::AnnotationUserdataDropped => "annotation.userdata-dropped",
            Self::ViewportUserdataDropped => "viewport.userdata-dropped",
            Self::MeshNgonGroupingDropped => "mesh.ngon-grouping-dropped",
            Self::EnumerationValueDegraded => "container.enumeration-value-degraded",
            Self::RedundantFieldRepaired => "container.redundant-field-repaired",
            Self::BrepMeshCacheDegraded => "brep.mesh-cache-degraded",
            Self::DuplicateRecordResolved => "container.duplicate-record-resolved",
            Self::MeshQuadTopologyTriangulated => "mesh.quad-topology-triangulated",
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
            Self::SourceWriterStampUnverified => "source.writer-stamp-unverified",
            Self::TopologyBodyKindGaugeSubstituted => "topology.body-kind-gauge-substituted",
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
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ContainerScanDiagnostic
            | Self::ContainerInstanceDefinitionDegraded
            | Self::ObjectFramingUndecodable
            | Self::ObjectDecodeDiagnostic
            | Self::AnnotationUserdataDropped
            | Self::PolycurveJoinGap
            | Self::ReferenceMemberUnresolved
            | Self::ReferenceMemberAmbiguous => LossTaxonomy::DecodeDiagnostic,
            Self::TrimPcurveDropped => LossTaxonomy::PcurveOmitted,
            Self::IntegrityFailure => LossTaxonomy::IntegrityFailure,
            Self::PresentationRecordDropped => LossTaxonomy::AssetNotTransferred,
            Self::ViewportUserdataDropped => LossTaxonomy::RecordNotTyped,
            Self::MeshNgonGroupingDropped | Self::MeshQuadTopologyTriangulated => {
                LossTaxonomy::RecordNotTyped
            }
            Self::EnumerationValueDegraded | Self::RedundantFieldRepaired => {
                LossTaxonomy::DecodeDiagnostic
            }
            Self::BrepMeshCacheDegraded => LossTaxonomy::RecordNotTyped,
            Self::DuplicateRecordResolved => LossTaxonomy::DecodeDiagnostic,
            Self::HistoryEmbeddedGeometryDropped => LossTaxonomy::GeometryNotTransferred,
            Self::DimensionOverrideDropped => LossTaxonomy::PmiOmitted,
            Self::HistoryDependencyDropped => LossTaxonomy::ReferenceGraphNotClosed,
            Self::ObjectRecordCensus => LossTaxonomy::ObjectRecordsUntransferred,
            Self::ObjectFamilyNotTransferred => LossTaxonomy::UnsupportedObjectFamily,
            Self::ObjectAttributesDegraded => LossTaxonomy::AttributesNotTransferred,
            Self::TopologyBrepFallback => LossTaxonomy::TopologyNotTransferred,
            Self::HatchFillNotTransferred
            | Self::PolyedgeReferencesNotResolved
            | Self::DetailViewNotTransferred
            | Self::CageLatticeNotTransferred
            | Self::MorphDeformationNotApplied
            | Self::CurveOnSurfaceBindingNotTransferred
            | Self::HistoryGeometryNotTransferred => LossTaxonomy::RecordNotTyped,
            Self::DimensionStyleUnresolved | Self::DimensionDetailReferenceUnresolved => {
                LossTaxonomy::PmiOmitted
            }
            Self::MeshVertexPrecisionReduced | Self::MeshNormalPrecisionReduced => {
                LossTaxonomy::MeshVertexPrecision
            }
            Self::SourceWriterStampUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::TopologyBodyKindGaugeSubstituted => LossTaxonomy::TopologyGaugeSubstituted,
        }
    }

    /// Strict floor pinned from this local code (independent of taxonomy remap).
    ///
    /// Defaults to the taxonomy floor so a later local→taxonomy remap cannot
    /// silently change rejection. `ObjectFramingUndecodable` pins Warning above
    /// its `DecodeDiagnostic` taxonomy (which is otherwise tolerable).
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::IntegrityFailure | Self::ObjectFramingUndecodable => Some(Severity::Warning),
            other => other.shared_taxonomy().strict_floor(),
        }
    }

    /// Namespaced [`LossKind`] for this local code (taxonomy + pinned floor).
    #[must_use]
    pub fn kind(self) -> LossKind {
        LossKind::namespaced("rhino", self.code(), self.shared_taxonomy())
            .with_strict_floor(self.strict_floor())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `rhino/<local>`; the message is the per-instance
    /// text only. Severity and strict floor come from the local code.
    #[must_use]
    pub fn note(self, message: impl std::fmt::Display) -> LossNote {
        LossNote::new(self.kind(), message.to_string()).with_severity(self.severity())
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
                "annotation.userdata-dropped",
                "viewport.userdata-dropped",
                "mesh.ngon-grouping-dropped",
                "container.enumeration-value-degraded",
                "container.redundant-field-repaired",
                "brep.mesh-cache-degraded",
                "container.duplicate-record-resolved",
                "mesh.quad-topology-triangulated",
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
                "source.writer-stamp-unverified",
                "topology.body-kind-gauge-substituted",
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
            assert_eq!(note.message, "x");
            assert_eq!(note.code.namespace(), "rhino");
            assert_eq!(note.code.local_code(), code.code());
            assert!(note.provenance.is_none());
        }
    }
}
