// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.prt` decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a stable
//! machine-readable code from [`NxLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`NxLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate: it fixes the shared loss code, the category, and
//! the severity from the variant so the three cannot drift apart across sites,
//! leaving only the per-instance message to the caller.
//!
//! Where two sites share a conceptual loss but disagree on severity, they are
//! kept as distinct variants rather than normalized: the two `DecodeDiagnostic`
//! deltas variants back the resolved (`Info`) and unresolved-tombstone
//! (`Warning`) outcomes of the same census so the fixed severity stays faithful
//! to the site it replaced.
//!
//! The vocabulary is crate-private: [`NxLossCode`] never appears in serialized
//! output — the [`LossNote`] carries the shared [`LossCode`] the variant maps
//! to — and no production caller outside this crate reads it.

use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};

/// A stable, machine-readable identifier for one `.prt` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`NxLossCode::code`]) is the stable contract; the Rust
/// variant name may be refactored freely.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum NxLossCode {
    /// A non-Parasolid preview stream was classified but not transferred.
    NonParasolidStreamOmitted,
    /// Census of the analytic point, surface, and curve carriers decoded.
    CarrierSummary,
    /// Census of the embedded JT display tessellations decoded.
    TessellationSummary,
    /// The B-rep topology graph did not form a complete ownership graph.
    TopologyGraphNotReconstructed,
    /// Surface-intersection records without a validated chart/term remain opaque.
    IntersectionRecordsOpaque,
    /// Deltas streams applied; every terminal tombstone resolved.
    DeltasApplied,
    /// Deltas streams applied, but some terminal tombstones stay unresolved.
    DeltasTombstonesUnresolved,
    /// Sub-body partition composition is not resolved to a terminal image.
    SubBodyCompositionUnresolved,
    /// Material, appearance, entity-attribute, and occurrence data not transferred.
    MaterialMetadataNotTransferred,
    /// Feature-history operation suppression state is unresolved.
    FeatureSuppressionUnresolved,
    /// Design configuration activation or body membership is unresolved.
    ConfigurationActivationUnresolved,
    /// Expression parameter evaluation or dependency semantics are incomplete.
    ExpressionParameterIncomplete,
    /// Feature-history operations remain native-only, without neutral semantics.
    FeatureNativeOnly,
    /// Feature family identity transferred, but neutral construction is unresolved.
    FeatureFamilyConstructionUnresolved,
    /// Typed feature family transferred, but construction or output lineage lags.
    FeatureFamilyLineageUnresolved,
    /// Sketch history feature carries no neutral sketch graph.
    SketchGraphUnresolved,
    /// Sketch records transferred, but no sketch constraints were projected.
    SketchConstraintsUntransferred,
    /// Sketch geometry or constraint records retain their native kind.
    SketchRecordsNative,
    /// Assembly `.prt`: component geometry lives in external child parts.
    AssemblyComponentsExternal,
    /// No gate-passing analytic carrier was found in the Parasolid streams.
    GeometryNotTransferred,
    /// Container-only decode requested; entity decode was not attempted.
    ContainerOnlyDecode,
}

impl NxLossCode {
    /// Every code, in declaration order. Used by tests to assert stability.
    #[cfg(test)]
    pub(crate) const ALL: &'static [NxLossCode] = &[
        Self::NonParasolidStreamOmitted,
        Self::CarrierSummary,
        Self::TessellationSummary,
        Self::TopologyGraphNotReconstructed,
        Self::IntersectionRecordsOpaque,
        Self::DeltasApplied,
        Self::DeltasTombstonesUnresolved,
        Self::SubBodyCompositionUnresolved,
        Self::MaterialMetadataNotTransferred,
        Self::FeatureSuppressionUnresolved,
        Self::ConfigurationActivationUnresolved,
        Self::ExpressionParameterIncomplete,
        Self::FeatureNativeOnly,
        Self::FeatureFamilyConstructionUnresolved,
        Self::FeatureFamilyLineageUnresolved,
        Self::SketchGraphUnresolved,
        Self::SketchConstraintsUntransferred,
        Self::SketchRecordsNative,
        Self::AssemblyComponentsExternal,
        Self::GeometryNotTransferred,
        Self::ContainerOnlyDecode,
    ];

    /// The stable string identifier. This is the gating contract.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::NonParasolidStreamOmitted => "stream.non-parasolid-omitted",
            Self::CarrierSummary => "carrier.summary",
            Self::TessellationSummary => "carrier.tessellation-summary",
            Self::TopologyGraphNotReconstructed => "topology.graph-not-reconstructed",
            Self::IntersectionRecordsOpaque => "geometry.intersection-records-opaque",
            Self::DeltasApplied => "topology.deltas-applied",
            Self::DeltasTombstonesUnresolved => "topology.deltas-tombstones-unresolved",
            Self::SubBodyCompositionUnresolved => "topology.sub-body-composition-unresolved",
            Self::MaterialMetadataNotTransferred => "attribute.material-metadata-not-transferred",
            Self::FeatureSuppressionUnresolved => "feature.suppression-unresolved",
            Self::ConfigurationActivationUnresolved => "config.activation-unresolved",
            Self::ExpressionParameterIncomplete => "parameter.expression-incomplete",
            Self::FeatureNativeOnly => "feature.native-only",
            Self::FeatureFamilyConstructionUnresolved => "feature.family-construction-unresolved",
            Self::FeatureFamilyLineageUnresolved => "feature.family-lineage-unresolved",
            Self::SketchGraphUnresolved => "sketch.graph-unresolved",
            Self::SketchConstraintsUntransferred => "sketch.constraints-untransferred",
            Self::SketchRecordsNative => "sketch.records-native",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::GeometryNotTransferred => "geometry.not-transferred",
            Self::ContainerOnlyDecode => "container.only-decode",
        }
    }

    /// The subsystem category this loss belongs to.
    #[must_use]
    pub(crate) const fn category(self) -> LossCategory {
        match self {
            Self::CarrierSummary
            | Self::TessellationSummary
            | Self::IntersectionRecordsOpaque
            | Self::AssemblyComponentsExternal
            | Self::GeometryNotTransferred
            | Self::ContainerOnlyDecode => LossCategory::Geometry,
            Self::TopologyGraphNotReconstructed
            | Self::DeltasApplied
            | Self::DeltasTombstonesUnresolved
            | Self::SubBodyCompositionUnresolved => LossCategory::Topology,
            Self::MaterialMetadataNotTransferred => LossCategory::Attribute,
            Self::FeatureSuppressionUnresolved
            | Self::ConfigurationActivationUnresolved
            | Self::ExpressionParameterIncomplete
            | Self::FeatureNativeOnly
            | Self::FeatureFamilyConstructionUnresolved
            | Self::FeatureFamilyLineageUnresolved
            | Self::SketchGraphUnresolved
            | Self::SketchConstraintsUntransferred
            | Self::SketchRecordsNative => LossCategory::DesignIntent,
            Self::NonParasolidStreamOmitted => LossCategory::Other,
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub(crate) const fn severity(self) -> Severity {
        match self {
            Self::NonParasolidStreamOmitted
            | Self::CarrierSummary
            | Self::TessellationSummary
            | Self::DeltasApplied
            | Self::ContainerOnlyDecode => Severity::Info,
            Self::TopologyGraphNotReconstructed
            | Self::AssemblyComponentsExternal
            | Self::GeometryNotTransferred => Severity::Blocking,
            _ => Severity::Warning,
        }
    }

    /// The shared IR loss code this variant serializes as.
    const fn shared_code(self) -> LossCode {
        match self {
            Self::NonParasolidStreamOmitted => LossCode::PassthroughRecordOmitted,
            Self::CarrierSummary | Self::TessellationSummary => LossCode::CarrierSummary,
            Self::TopologyGraphNotReconstructed => LossCode::TopologyNotTransferred,
            Self::IntersectionRecordsOpaque => LossCode::ObjectRecordsUntransferred,
            Self::DeltasApplied | Self::DeltasTombstonesUnresolved => LossCode::DecodeDiagnostic,
            Self::SubBodyCompositionUnresolved
            | Self::FeatureSuppressionUnresolved
            | Self::ConfigurationActivationUnresolved
            | Self::ExpressionParameterIncomplete
            | Self::FeatureNativeOnly
            | Self::FeatureFamilyConstructionUnresolved
            | Self::FeatureFamilyLineageUnresolved
            | Self::SketchGraphUnresolved
            | Self::SketchConstraintsUntransferred
            | Self::SketchRecordsNative => LossCode::FeatureHistoryRetained,
            Self::MaterialMetadataNotTransferred => LossCode::AttributesNotTransferred,
            Self::AssemblyComponentsExternal => LossCode::AssemblyComponentsExternal,
            Self::GeometryNotTransferred => LossCode::GeometryNotTransferred,
            Self::ContainerOnlyDecode => LossCode::ContainerOnly,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The shared code, category, and severity come from the variant, so a site
    /// cannot mislabel a loss it names. Provenance is left absent; the decoder
    /// attributes losses through the message and record identity, not a source
    /// span.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote {
            code: self.shared_code(),
            category: self.category(),
            severity: self.severity(),
            message: message.into(),
            provenance: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NxLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned. A diff
    /// here is an intentional contract change to a gating identifier.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = NxLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "stream.non-parasolid-omitted",
                "carrier.summary",
                "carrier.tessellation-summary",
                "topology.graph-not-reconstructed",
                "geometry.intersection-records-opaque",
                "topology.deltas-applied",
                "topology.deltas-tombstones-unresolved",
                "topology.sub-body-composition-unresolved",
                "attribute.material-metadata-not-transferred",
                "feature.suppression-unresolved",
                "config.activation-unresolved",
                "parameter.expression-incomplete",
                "feature.native-only",
                "feature.family-construction-unresolved",
                "feature.family-lineage-unresolved",
                "sketch.graph-unresolved",
                "sketch.constraints-untransferred",
                "sketch.records-native",
                "assembly.components-external",
                "geometry.not-transferred",
                "container.only-decode",
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

    /// The note builder fixes the shared code, category, and severity from the
    /// variant so a call site cannot mislabel a loss it names.
    #[test]
    fn note_takes_code_category_and_severity_from_the_variant() {
        for code in NxLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.code, code.shared_code());
            assert_eq!(note.category, code.category());
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
