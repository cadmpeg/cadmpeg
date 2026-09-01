// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.sldprt` decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`SldprtLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`SldprtLossCode::note`] is the single practical construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller.
//!
use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};

/// A stable, machine-readable identifier for one `.sldprt` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`SldprtLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SldprtLossCode {
    /// Active configuration identity does not resolve to exactly one record.
    ConfigActiveIdentityUnresolved,
    /// Active configuration does not resolve to the active geometry partition.
    ConfigActivePartitionMismatch,
    /// Configuration state inferred from geometry without a native definition.
    ConfigInferredWithoutNative,
    /// Configuration-scoped feature-input lane has unresolved configuration id.
    ConfigLaneIdentityUnresolved,
    /// Configuration records share non-unique geometry partition identities.
    ConfigAmbiguousPartition,
    /// Configuration records have empty, duplicate, or colliding names/ordinals.
    ConfigAmbiguousNaming,
    /// Configuration record references missing or repeated bodies.
    ConfigIncoherentBodyRefs,
    /// Configuration lacks a complete evaluated feature/parameter snapshot.
    ConfigIncompleteSnapshot,
    /// Parameter lacks an evaluated scalar or a regenerable finite value.
    ParameterUnevaluated,
    /// Parameter records have empty, duplicate, or colliding names/ordinals.
    ParameterAmbiguousIdentity,
    /// Semantic dimension record is unbound or retains a native subtype.
    PmiDimensionUnbound,
    /// `PMISemanticDataDB` `MessagePack` map failed to parse into a dimension.
    PmiSemanticRecordMalformed,
    /// SWIFT annotation class has no format-neutral PMI representation.
    PmiSwiftAnnotationUnsupported,
    /// Feature history record has duplicate identity or unresolved references.
    HistoryIncompleteReferences,
    /// Feature record has missing, repeated, or non-preceding tree edges.
    FeatureIncoherentEdges,
    /// Feature record has inconsistent source-content references.
    FeatureIncoherentContent,
    /// Feature retains a native output scope that does not resolve to a body.
    FeatureUnresolvedOutputScope,
    /// Feature record has missing or repeated output body references.
    FeatureIncoherentOutputs,
    /// Sketch constraint retains a native relation kind without neutral semantics.
    SketchNativeConstraint,
    /// Sketch geometry record retains a native kind without solved geometry.
    SketchNativeGeometry,
    /// Native sketch relation has no projected neutral constraint.
    SketchRelationUnprojected,
    /// Native sketch relation is claimed by multiple neutral objects.
    SketchRelationMultiplyProjected,
    /// Feature retains its native kind without a complete neutral operation.
    FeatureNativeKindRetained,
    /// Native feature-input operation object does not bind uniquely to a feature.
    FeatureInputObjectUnbound,
    /// Typed feature retains native or unresolved required operation operands.
    FeatureTypedOperandIncomplete,
    /// Body delete/keep feature retains native selection without a decoded mode.
    FeatureBodyRetentionUnresolved,
    /// Face or procedural construction references an untyped support surface.
    GeometryFaceSupportSurfaceUntyped,
    /// Edge references an untyped support curve carried opaque.
    GeometryEdgeSupportCurveUntyped,
    /// A derived pcurve parameter has multiple geometric candidates.
    GeometryPcurveAmbiguous,
    /// Current face records carry conflicting or incoherent color bindings.
    AppearanceFaceColorUnresolved,
    /// Appearance assignments have no unambiguous `DisplayLists` target.
    AppearanceAssignmentUnresolved,
    /// `DisplayLists` tessellation does not resolve to its B-rep face owners.
    TessellationFaceOwnershipUnresolved,
    /// No body record was available; a body hierarchy was derived.
    TopologyBodyHierarchyDerived,
    /// One face owner has multiple non-equivalent face-use bridges.
    TopologyFaceOwnerAmbiguous,
    /// A canonical face has no explicit body relation.
    TopologyFaceUnclaimed,
    /// A NURBS edge's vertex range is off its bound surface; no pcurve is derived.
    TopologyPcurveCarrierOffSurface,
    /// Parasolid B-rep geometry was not transferred (no resolved stream).
    GeometryParasolidNotTransferred,
    /// B-rep topology graph was not built for this file.
    TopologyGraphNotTransferred,
    /// Materials, tessellation, and metadata were not transferred.
    MaterialMetadataNotTransferred,
    /// No Parasolid partition/deltas stream was located in the container.
    ContainerNoParasolidStream,
    /// Preserved source image required for a byte-exact write was unavailable.
    SourcePreservedImageUnavailable,
    /// The document carries no usable `swVersion`, so no declared identity was
    /// verified.
    ///
    /// Charged exactly when the primary-layer [`crate::dialect`] match is
    /// `Admission::AdmittedUnverified`, from the same predicate that decides
    /// the admission. A residual `unknown` row is the absence of a declared
    /// identity, and admission verifies a declared identity, so the pair
    /// (`sldprt:unknown`, `Admitted`) is unreachable: a part that declares
    /// nothing must be distinguishable from one whose declaration was
    /// verified.
    SourceDialectUnverified,
    /// An embedded Parasolid schema has no declared grammar and was recovered as residual.
    KernelDialectUnverified,
    /// Two embedded kernel carriers resolved to one dialect-layer identity.
    DialectLayerCollision,
    /// The selected write target differs from the same-format source dialect.
    SourceDialectDisplaced,
}

impl SldprtLossCode {
    /// Every code, in declaration order.
    pub const ALL: &'static [SldprtLossCode] = &[
        Self::ConfigActiveIdentityUnresolved,
        Self::ConfigActivePartitionMismatch,
        Self::ConfigInferredWithoutNative,
        Self::ConfigLaneIdentityUnresolved,
        Self::ConfigAmbiguousPartition,
        Self::ConfigAmbiguousNaming,
        Self::ConfigIncoherentBodyRefs,
        Self::ConfigIncompleteSnapshot,
        Self::ParameterUnevaluated,
        Self::ParameterAmbiguousIdentity,
        Self::PmiDimensionUnbound,
        Self::PmiSemanticRecordMalformed,
        Self::PmiSwiftAnnotationUnsupported,
        Self::HistoryIncompleteReferences,
        Self::FeatureIncoherentEdges,
        Self::FeatureIncoherentContent,
        Self::FeatureUnresolvedOutputScope,
        Self::FeatureIncoherentOutputs,
        Self::SketchNativeConstraint,
        Self::SketchNativeGeometry,
        Self::SketchRelationUnprojected,
        Self::SketchRelationMultiplyProjected,
        Self::FeatureNativeKindRetained,
        Self::FeatureInputObjectUnbound,
        Self::FeatureTypedOperandIncomplete,
        Self::FeatureBodyRetentionUnresolved,
        Self::GeometryFaceSupportSurfaceUntyped,
        Self::GeometryEdgeSupportCurveUntyped,
        Self::GeometryPcurveAmbiguous,
        Self::AppearanceFaceColorUnresolved,
        Self::AppearanceAssignmentUnresolved,
        Self::TessellationFaceOwnershipUnresolved,
        Self::TopologyBodyHierarchyDerived,
        Self::TopologyFaceOwnerAmbiguous,
        Self::TopologyFaceUnclaimed,
        Self::TopologyPcurveCarrierOffSurface,
        Self::GeometryParasolidNotTransferred,
        Self::TopologyGraphNotTransferred,
        Self::MaterialMetadataNotTransferred,
        Self::ContainerNoParasolidStream,
        Self::SourcePreservedImageUnavailable,
        Self::SourceDialectUnverified,
        Self::KernelDialectUnverified,
        Self::DialectLayerCollision,
        Self::SourceDialectDisplaced,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ConfigActiveIdentityUnresolved => "config.active-identity-unresolved",
            Self::ConfigActivePartitionMismatch => "config.active-partition-mismatch",
            Self::ConfigInferredWithoutNative => "config.inferred-without-native",
            Self::ConfigLaneIdentityUnresolved => "config.lane-identity-unresolved",
            Self::ConfigAmbiguousPartition => "config.ambiguous-partition",
            Self::ConfigAmbiguousNaming => "config.ambiguous-naming",
            Self::ConfigIncoherentBodyRefs => "config.incoherent-body-refs",
            Self::ConfigIncompleteSnapshot => "config.incomplete-snapshot",
            Self::ParameterUnevaluated => "parameter.unevaluated",
            Self::ParameterAmbiguousIdentity => "parameter.ambiguous-identity",
            Self::PmiDimensionUnbound => "pmi.dimension-unbound",
            Self::PmiSemanticRecordMalformed => "pmi.semantic-record-malformed",
            Self::PmiSwiftAnnotationUnsupported => "pmi.swift-annotation-unsupported",
            Self::HistoryIncompleteReferences => "history.incomplete-references",
            Self::FeatureIncoherentEdges => "feature.incoherent-edges",
            Self::FeatureIncoherentContent => "feature.incoherent-content",
            Self::FeatureUnresolvedOutputScope => "feature.unresolved-output-scope",
            Self::FeatureIncoherentOutputs => "feature.incoherent-outputs",
            Self::SketchNativeConstraint => "sketch.native-constraint",
            Self::SketchNativeGeometry => "sketch.native-geometry",
            Self::SketchRelationUnprojected => "sketch.relation-unprojected",
            Self::SketchRelationMultiplyProjected => "sketch.relation-multiply-projected",
            Self::FeatureNativeKindRetained => "feature.native-kind-retained",
            Self::FeatureInputObjectUnbound => "feature.input-object-unbound",
            Self::FeatureTypedOperandIncomplete => "feature.typed-operand-incomplete",
            Self::FeatureBodyRetentionUnresolved => "feature.body-retention-unresolved",
            Self::GeometryFaceSupportSurfaceUntyped => "geometry.face-support-surface-untyped",
            Self::GeometryEdgeSupportCurveUntyped => "geometry.edge-support-curve-untyped",
            Self::GeometryPcurveAmbiguous => "geometry.pcurve-ambiguous",
            Self::AppearanceFaceColorUnresolved => "appearance.face-color-unresolved",
            Self::AppearanceAssignmentUnresolved => "appearance.assignment-unresolved",
            Self::TessellationFaceOwnershipUnresolved => "tessellation.face-ownership-unresolved",
            Self::TopologyBodyHierarchyDerived => "topology.body-hierarchy-derived",
            Self::TopologyFaceOwnerAmbiguous => "topology.face-owner-ambiguous",
            Self::TopologyFaceUnclaimed => "topology.face-unclaimed",
            Self::TopologyPcurveCarrierOffSurface => "topology.pcurve-carrier-off-surface",
            Self::GeometryParasolidNotTransferred => "geometry.parasolid-not-transferred",
            Self::TopologyGraphNotTransferred => "topology.graph-not-transferred",
            Self::MaterialMetadataNotTransferred => "material.metadata-not-transferred",
            Self::ContainerNoParasolidStream => "container.no-parasolid-stream",
            Self::SourcePreservedImageUnavailable => "source.preserved-image-unavailable",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::DialectLayerCollision => "source.dialect-layer-collision",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::GeometryParasolidNotTransferred
            | Self::TopologyGraphNotTransferred
            | Self::SourcePreservedImageUnavailable => Severity::Blocking,
            Self::ContainerNoParasolidStream => Severity::Error,
            _ => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ContainerNoParasolidStream => LossTaxonomy::MissingGeometryStream,
            Self::SourcePreservedImageUnavailable => LossTaxonomy::PreservedSourceUnavailable,
            Self::SourceDialectUnverified | Self::KernelDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::DialectLayerCollision => LossTaxonomy::DecodeDiagnostic,
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::TopologyBodyHierarchyDerived | Self::TopologyFaceOwnerAmbiguous => {
                LossTaxonomy::TopologyGaugeSubstituted
            }
            Self::TopologyFaceUnclaimed => LossTaxonomy::TopologyNotTransferred,
            Self::TopologyPcurveCarrierOffSurface => LossTaxonomy::PcurveOmitted,
            Self::TopologyGraphNotTransferred => LossTaxonomy::TopologyNotTransferred,
            Self::GeometryFaceSupportSurfaceUntyped
            | Self::GeometryEdgeSupportCurveUntyped
            | Self::GeometryParasolidNotTransferred => LossTaxonomy::GeometryNotTransferred,
            Self::GeometryPcurveAmbiguous => LossTaxonomy::PcurveOmitted,
            Self::AppearanceFaceColorUnresolved | Self::AppearanceAssignmentUnresolved => {
                LossTaxonomy::MaterialNotTransferred
            }
            Self::TessellationFaceOwnershipUnresolved => LossTaxonomy::ReferenceGraphNotClosed,
            Self::MaterialMetadataNotTransferred => LossTaxonomy::MaterialNotTransferred,
            _ => LossTaxonomy::FeatureHistoryRetained,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub fn kind(self) -> LossKind {
        LossKind::namespaced("sldprt", self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `sldprt/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::report::{LossTaxonomy, StrictConsequence};

    use super::SldprtLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = SldprtLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "config.active-identity-unresolved",
                "config.active-partition-mismatch",
                "config.inferred-without-native",
                "config.lane-identity-unresolved",
                "config.ambiguous-partition",
                "config.ambiguous-naming",
                "config.incoherent-body-refs",
                "config.incomplete-snapshot",
                "parameter.unevaluated",
                "parameter.ambiguous-identity",
                "pmi.dimension-unbound",
                "pmi.semantic-record-malformed",
                "pmi.swift-annotation-unsupported",
                "history.incomplete-references",
                "feature.incoherent-edges",
                "feature.incoherent-content",
                "feature.unresolved-output-scope",
                "feature.incoherent-outputs",
                "sketch.native-constraint",
                "sketch.native-geometry",
                "sketch.relation-unprojected",
                "sketch.relation-multiply-projected",
                "feature.native-kind-retained",
                "feature.input-object-unbound",
                "feature.typed-operand-incomplete",
                "feature.body-retention-unresolved",
                "geometry.face-support-surface-untyped",
                "geometry.edge-support-curve-untyped",
                "geometry.pcurve-ambiguous",
                "appearance.face-color-unresolved",
                "appearance.assignment-unresolved",
                "tessellation.face-ownership-unresolved",
                "topology.body-hierarchy-derived",
                "topology.face-owner-ambiguous",
                "topology.face-unclaimed",
                "topology.pcurve-carrier-off-surface",
                "geometry.parasolid-not-transferred",
                "topology.graph-not-transferred",
                "material.metadata-not-transferred",
                "container.no-parasolid-stream",
                "source.preserved-image-unavailable",
                "source.dialect-unverified",
                "source.kernel-dialect-unverified",
                "source.dialect-layer-collision",
                "target.source-dialect-displaced",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in SldprtLossCode::ALL {
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

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in SldprtLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }

    #[test]
    fn ambiguous_pcurve_is_a_tolerable_pcurve_omission() {
        let note = SldprtLossCode::GeometryPcurveAmbiguous.note("ambiguous");
        assert_eq!(note.code.namespace(), "sldprt");
        assert_eq!(note.code.local_code(), "geometry.pcurve-ambiguous");
        assert_eq!(note.code.taxonomy(), LossTaxonomy::PcurveOmitted);
        assert_eq!(note.strict_consequence(), StrictConsequence::Tolerate);
    }
}
