// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for NX `.prt` decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`NxLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`NxLossCode::note`] is the single practical construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `nx` namespace.
//!
//! [`NxLossCode::shared_taxonomy`] is an exhaustive match with no fall-through
//! arm. A default arm would silently assign a category to a code added later,
//! and the categories this codec spans (carrier, topology, history, container)
//! have no honest common default.

use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};

/// A stable, machine-readable identifier for one NX `.prt` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`NxLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NxLossCode {
    /// An embedded kernel dialect had no declared grammar and was recovered as residual.
    KernelDialectUnverified,
    /// Two embedded kernel carriers resolved to one dialect-layer identity.
    DialectLayerCollision,
    /// Census of decoded Parasolid POINT and analytic curve/surface carriers.
    CarrierAnalyticCensus,
    /// Census of decoded embedded JT display tessellations.
    CarrierTessellationCensus,
    /// B-rep topology graph was not reconstructed from surviving typed records.
    TopologyGraphNotReconstructed,
    /// Surface-intersection records lack a validated chart and term-endpoint witness.
    IntersectionRecordsOpaque,
    /// Geometric completion reached a declared work bound before all
    /// intersection pcurve lanes were complete.
    IntersectionPcurveCompletionBounded,
    /// Adaptive geometry certification reached its model-wide work bound.
    GeometryAdaptiveWorkBounded,
    /// Parasolid deltas applied; every terminal tombstone resolved to a key.
    DeltasApplied,
    /// Parasolid deltas applied; one or more terminal tombstones remain unmatched.
    DeltasUnmatchedTombstones,
    /// Sub-body partitions remain; Boolean history does not resolve every image.
    SubBodyCompositionUnresolved,
    /// A referenced Parasolid attribute value relation did not resolve.
    AttributeValueUnresolved,
    /// Feature-history suppression state remains unresolved.
    FeatureSuppressionUnresolved,
    /// Configuration activation, body membership, or evaluated state is incomplete.
    ConfigurationStateUnresolved,
    /// Expression parameter evaluation or dependency semantics are incomplete.
    ExpressionParameterIncomplete,
    /// Feature-history operations remain native-only without neutral semantics.
    FeatureNativeKindRetained,
    /// Feature family identities transferred; construction semantics unresolved.
    FeatureFamilyConstructionUnresolved,
    /// Typed feature output lineage is missing, duplicated, or unresolved.
    FeatureOutputLineageIncomplete,
    /// Typed feature operations have incomplete neutral construction fields.
    FeatureConstructionIncomplete,
    /// Sketch history features have no neutral sketch graph.
    SketchGraphUnresolved,
    /// Sketch geometry or constraint records remain native-only.
    SketchNativeSemantics,
    /// Bounded offset-store control blocks have no admitted complete grammar.
    OffsetStoreControlUntyped,
    /// A named container stream is retained byte-exact without typed fields.
    ContainerStreamOpaque,
    /// A classified non-Parasolid stream was not transferred.
    NonParasolidStreamOmitted,
    /// Assembly `.prt` has no inline geometry; children live in external parts.
    AssemblyComponentsExternal,
    /// No gate-passing analytic carrier was found in the Parasolid streams.
    GeometryNotTransferred,
}

impl NxLossCode {
    /// Every code, in declaration order.
    pub const ALL: &'static [NxLossCode] = &[
        Self::KernelDialectUnverified,
        Self::DialectLayerCollision,
        Self::CarrierAnalyticCensus,
        Self::CarrierTessellationCensus,
        Self::TopologyGraphNotReconstructed,
        Self::IntersectionRecordsOpaque,
        Self::IntersectionPcurveCompletionBounded,
        Self::GeometryAdaptiveWorkBounded,
        Self::DeltasApplied,
        Self::DeltasUnmatchedTombstones,
        Self::SubBodyCompositionUnresolved,
        Self::AttributeValueUnresolved,
        Self::FeatureSuppressionUnresolved,
        Self::ConfigurationStateUnresolved,
        Self::ExpressionParameterIncomplete,
        Self::FeatureNativeKindRetained,
        Self::FeatureFamilyConstructionUnresolved,
        Self::FeatureOutputLineageIncomplete,
        Self::FeatureConstructionIncomplete,
        Self::SketchGraphUnresolved,
        Self::SketchNativeSemantics,
        Self::OffsetStoreControlUntyped,
        Self::ContainerStreamOpaque,
        Self::NonParasolidStreamOmitted,
        Self::AssemblyComponentsExternal,
        Self::GeometryNotTransferred,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::DialectLayerCollision => "source.dialect-layer-collision",
            Self::CarrierAnalyticCensus => "carrier.analytic-census",
            Self::CarrierTessellationCensus => "carrier.tessellation-census",
            Self::TopologyGraphNotReconstructed => "topology.graph-not-reconstructed",
            Self::IntersectionRecordsOpaque => "intersection.records-opaque",
            Self::IntersectionPcurveCompletionBounded => "intersection.pcurve-completion-bounded",
            Self::GeometryAdaptiveWorkBounded => "geometry.adaptive-work-bounded",
            Self::DeltasApplied => "deltas.applied",
            Self::DeltasUnmatchedTombstones => "deltas.unmatched-tombstones",
            Self::SubBodyCompositionUnresolved => "history.sub-body-composition-unresolved",
            Self::AttributeValueUnresolved => "attribute.value-unresolved",
            Self::FeatureSuppressionUnresolved => "feature.suppression-unresolved",
            Self::ConfigurationStateUnresolved => "configuration.state-unresolved",
            Self::ExpressionParameterIncomplete => "expression.parameter-incomplete",
            Self::FeatureNativeKindRetained => "feature.native-kind-retained",
            Self::FeatureFamilyConstructionUnresolved => "feature.family-construction-unresolved",
            Self::FeatureOutputLineageIncomplete => "feature.output-lineage-incomplete",
            Self::FeatureConstructionIncomplete => "feature.construction-incomplete",
            Self::SketchGraphUnresolved => "sketch.graph-unresolved",
            Self::SketchNativeSemantics => "sketch.native-semantics",
            Self::OffsetStoreControlUntyped => "container.offset-store-control-untyped",
            Self::ContainerStreamOpaque => "container.stream-opaque",
            Self::NonParasolidStreamOmitted => "stream.non-parasolid-omitted",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::GeometryNotTransferred => "geometry.not-transferred",
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::CarrierAnalyticCensus
            | Self::CarrierTessellationCensus
            | Self::DeltasApplied
            | Self::ContainerStreamOpaque
            | Self::NonParasolidStreamOmitted => Severity::Info,
            Self::TopologyGraphNotReconstructed
            | Self::AssemblyComponentsExternal
            | Self::GeometryNotTransferred => Severity::Blocking,
            Self::KernelDialectUnverified
            | Self::DialectLayerCollision
            | Self::IntersectionRecordsOpaque
            | Self::IntersectionPcurveCompletionBounded
            | Self::GeometryAdaptiveWorkBounded
            | Self::DeltasUnmatchedTombstones
            | Self::SubBodyCompositionUnresolved
            | Self::AttributeValueUnresolved
            | Self::FeatureSuppressionUnresolved
            | Self::ConfigurationStateUnresolved
            | Self::ExpressionParameterIncomplete
            | Self::FeatureNativeKindRetained
            | Self::FeatureFamilyConstructionUnresolved
            | Self::FeatureOutputLineageIncomplete
            | Self::FeatureConstructionIncomplete
            | Self::SketchGraphUnresolved
            | Self::SketchNativeSemantics
            | Self::OffsetStoreControlUntyped => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::KernelDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::DialectLayerCollision => LossTaxonomy::DecodeDiagnostic,
            Self::CarrierAnalyticCensus | Self::CarrierTessellationCensus => {
                LossTaxonomy::CarrierSummary
            }
            Self::TopologyGraphNotReconstructed => LossTaxonomy::TopologyNotTransferred,
            Self::IntersectionRecordsOpaque
            | Self::IntersectionPcurveCompletionBounded
            | Self::GeometryAdaptiveWorkBounded => LossTaxonomy::ObjectRecordsUntransferred,
            Self::DeltasApplied | Self::DeltasUnmatchedTombstones => LossTaxonomy::DecodeDiagnostic,
            Self::SubBodyCompositionUnresolved
            | Self::FeatureSuppressionUnresolved
            | Self::ConfigurationStateUnresolved
            | Self::ExpressionParameterIncomplete
            | Self::FeatureNativeKindRetained
            | Self::FeatureFamilyConstructionUnresolved
            | Self::FeatureOutputLineageIncomplete
            | Self::FeatureConstructionIncomplete
            | Self::SketchGraphUnresolved
            | Self::SketchNativeSemantics => LossTaxonomy::FeatureHistoryRetained,
            Self::AttributeValueUnresolved => LossTaxonomy::AttributesNotTransferred,
            Self::OffsetStoreControlUntyped | Self::ContainerStreamOpaque => {
                LossTaxonomy::RecordNotTyped
            }
            Self::NonParasolidStreamOmitted => LossTaxonomy::PassthroughRecordOmitted,
            Self::AssemblyComponentsExternal => LossTaxonomy::AssemblyComponentsExternal,
            Self::GeometryNotTransferred => LossTaxonomy::GeometryNotTransferred,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub fn kind(self) -> LossKind {
        LossKind::namespaced("nx", self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `nx/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::NxLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = NxLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "source.kernel-dialect-unverified",
                "source.dialect-layer-collision",
                "carrier.analytic-census",
                "carrier.tessellation-census",
                "topology.graph-not-reconstructed",
                "intersection.records-opaque",
                "intersection.pcurve-completion-bounded",
                "geometry.adaptive-work-bounded",
                "deltas.applied",
                "deltas.unmatched-tombstones",
                "history.sub-body-composition-unresolved",
                "attribute.value-unresolved",
                "feature.suppression-unresolved",
                "configuration.state-unresolved",
                "expression.parameter-incomplete",
                "feature.native-kind-retained",
                "feature.family-construction-unresolved",
                "feature.output-lineage-incomplete",
                "feature.construction-incomplete",
                "sketch.graph-unresolved",
                "sketch.native-semantics",
                "container.offset-store-control-untyped",
                "container.stream-opaque",
                "stream.non-parasolid-omitted",
                "assembly.components-external",
                "geometry.not-transferred",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in NxLossCode::ALL {
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
        for code in NxLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
