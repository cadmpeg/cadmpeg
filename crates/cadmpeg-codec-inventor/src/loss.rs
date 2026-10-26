// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for Inventor IPT/IAM decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`InventorLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`InventorLossCode::note`] is the single practical construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `inventor` namespace.
//!
//! [`InventorLossCode::shared_taxonomy`] is an exhaustive match with no
//! fall-through arm. A default arm would silently assign a category to a code
//! added later, and the categories this codec spans have no honest common
//! default.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNote, LossTaxonomy},
    Severity,
};

/// A stable, machine-readable identifier for one Inventor transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`InventorLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum InventorLossCode {
    /// The active kernel carrier was not transferred into neutral geometry.
    GeometryKernelCarrierNotTransferred,
    /// Faces use procedural surfaces without a decoded carrier.
    GeometryProceduralSurfaceNotTransferred,
    /// Structurally paired `RSe` segments have no record semantics.
    RseSegmentPairUntyped,
    /// `RSe` metadata streams are malformed or outside the implemented envelope.
    RseMetadataStreamMalformed,
    /// `RSe` bulk streams have invalid envelope or zlib framing.
    RseBulkStreamMalformed,
    /// Typed assembly records are malformed or outside the implemented branch.
    AssemblyRecordMalformed,
    /// Typed presentation records are malformed or outside the implemented branch.
    PresentationRecordMalformed,
    /// Typed design records are malformed or outside the implemented branch.
    DesignRecordMalformed,
    /// Typed sketch records could not be parsed exactly.
    SketchRecordMalformed,
    /// Typed feature records could not be parsed exactly.
    FeatureRecordMalformed,
    /// An optional feature label is empty and has no neutral display name.
    FeatureLabelUnusable,
    /// Sketch curves carry entity flags whose construction meaning is
    /// unresolved.
    SketchConstructionUnresolved,
    /// A sketch-point paired association has no neutral relation semantics.
    SketchPairedAssociationUnprojected,
    /// Profile-body records do not decode, so the profiles that select them
    /// stay native.
    FeatureProfileBodyUndecoded,
    /// A feature label name stores unpaired UTF-16 surrogates, which the
    /// neutral name replaces.
    FeatureLabelNameReplaced,
    /// Typed feature records have an operation graph that is not closed.
    FeatureOperationGraphOpen,
    /// Thread annotations retain sizing and termination operands only natively.
    FeatureThreadDefinitionPartial,
    /// Thicken selections and thickness side remain native operands.
    FeatureThickenOperandsNative,
    /// Shell operations retain construction controls only natively.
    FeatureShellDefinitionPartial,
    /// Attached plates have no neutral sheet-metal attached-plate kind.
    FeatureSheetMetalAttachedPlateKindMissing,
    /// Separate bend history records have no neutral sheet-metal bend kind.
    FeatureSheetMetalBendKindMissing,
    /// Inventor operations retain unresolved suppression and dependency state.
    FeatureStateUnresolved,
    /// Parameter records have a unit or expression graph that is not closed.
    ParameterGraphOpen,
    /// Parameter records have an expression whose text passes the local cap.
    ParameterExpressionBounded,
    /// Parameter records whose parameter table has no established owning model.
    ParameterScopeUnresolved,
    /// Sketch, entity, or constraint records have a neutral graph that is not closed.
    SketchGraphOpen,
    /// `RSe` contains unpaired metadata or bulk streams.
    RseStreamUnpaired,
    /// OLE property-set streams are malformed.
    PropertySetStreamMalformed,
    /// Property values have no neutral metadata mapping.
    MetadataPropertyUnmapped,
    /// The Protein asset catalog could not be decoded.
    ProteinCatalogUndecodable,
    /// Malformed Protein asset records were rejected.
    ProteinAssetRejected,
    /// The Protein package contains no decoded appearance assets.
    ProteinAppearanceAbsent,
    /// `PmApp` document-default appearance assignments did not resolve.
    AppearanceDefaultUnresolved,
    /// Protein catalog asset GUIDs collide; ambiguous texture joins were refused.
    ProteinGuidAmbiguous,
    /// A Protein texture states a distance with no length conversion.
    MaterialDistanceUnitUntyped,
    /// The Inventor Protein stream is malformed.
    ProteinStreamMalformed,
    /// `PmGraphics` face appearance overrides did not resolve.
    AppearanceFaceOverrideUnresolved,
    /// The `UFRxDoc` external-reference table is malformed.
    UfrxTableMalformed,
    /// An unsupported `UFRxDoc` schema was retained on an assembly document.
    UfrxSchemaUnsupportedAssembly,
    /// An unsupported `UFRxDoc` schema was retained without a typed transfer.
    UfrxSchemaUnsupported,
    /// External component references remain unresolved.
    AssemblyComponentExternal,
    /// Assembly occurrence placements could not be transferred.
    AssemblyPlacementNotTransferred,
    /// The document was read with a grammar its own declarations do not select.
    SourceDialectUnverified,
    /// The active kernel carrier used an unverified Spatial ACIS grammar band.
    KernelDialectUnverified,
    /// The selected kernel carrier did not expose a parseable kernel header.
    KernelCarrierUnparseable,
    /// An independently framed kernel product string is unreadable.
    KernelHeaderMetadataUnresolved,
    /// An optional kernel tolerance cannot enter document units.
    KernelHeaderToleranceUnresolved,
    /// The compound container uses an admitted noncanonical encoding.
    ContainerNoncanonical,
}

impl InventorLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [InventorLossCode] = &[
        Self::GeometryKernelCarrierNotTransferred,
        Self::GeometryProceduralSurfaceNotTransferred,
        Self::RseSegmentPairUntyped,
        Self::RseMetadataStreamMalformed,
        Self::RseBulkStreamMalformed,
        Self::AssemblyRecordMalformed,
        Self::PresentationRecordMalformed,
        Self::DesignRecordMalformed,
        Self::SketchRecordMalformed,
        Self::FeatureRecordMalformed,
        Self::FeatureLabelUnusable,
        Self::SketchConstructionUnresolved,
        Self::SketchPairedAssociationUnprojected,
        Self::FeatureProfileBodyUndecoded,
        Self::FeatureLabelNameReplaced,
        Self::FeatureOperationGraphOpen,
        Self::FeatureThreadDefinitionPartial,
        Self::FeatureThickenOperandsNative,
        Self::FeatureShellDefinitionPartial,
        Self::FeatureSheetMetalAttachedPlateKindMissing,
        Self::FeatureSheetMetalBendKindMissing,
        Self::FeatureStateUnresolved,
        Self::ParameterGraphOpen,
        Self::ParameterExpressionBounded,
        Self::ParameterScopeUnresolved,
        Self::SketchGraphOpen,
        Self::RseStreamUnpaired,
        Self::PropertySetStreamMalformed,
        Self::MetadataPropertyUnmapped,
        Self::ProteinCatalogUndecodable,
        Self::ProteinAssetRejected,
        Self::ProteinAppearanceAbsent,
        Self::AppearanceDefaultUnresolved,
        Self::ProteinGuidAmbiguous,
        Self::MaterialDistanceUnitUntyped,
        Self::ProteinStreamMalformed,
        Self::AppearanceFaceOverrideUnresolved,
        Self::UfrxTableMalformed,
        Self::UfrxSchemaUnsupportedAssembly,
        Self::UfrxSchemaUnsupported,
        Self::AssemblyComponentExternal,
        Self::AssemblyPlacementNotTransferred,
        Self::SourceDialectUnverified,
        Self::KernelDialectUnverified,
        Self::KernelCarrierUnparseable,
        Self::KernelHeaderMetadataUnresolved,
        Self::KernelHeaderToleranceUnresolved,
        Self::ContainerNoncanonical,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::GeometryKernelCarrierNotTransferred => "geometry.kernel-carrier-not-transferred",
            Self::GeometryProceduralSurfaceNotTransferred => {
                "geometry.procedural-surface-not-transferred"
            }
            Self::RseSegmentPairUntyped => "rse.segment-pair-untyped",
            Self::RseMetadataStreamMalformed => "rse.metadata-stream-malformed",
            Self::RseBulkStreamMalformed => "rse.bulk-stream-malformed",
            Self::AssemblyRecordMalformed => "assembly.record-malformed",
            Self::PresentationRecordMalformed => "presentation.record-malformed",
            Self::DesignRecordMalformed => "design.record-malformed",
            Self::SketchRecordMalformed => "sketch.record-malformed",
            Self::FeatureRecordMalformed => "feature.record-malformed",
            Self::FeatureLabelUnusable => "feature.label-unusable",
            Self::SketchConstructionUnresolved => "sketch.construction-unresolved",
            Self::SketchPairedAssociationUnprojected => "sketch.paired-association-unprojected",
            Self::FeatureProfileBodyUndecoded => "feature.profile-body-undecoded",
            Self::FeatureLabelNameReplaced => "feature.label-name-replaced",
            Self::FeatureOperationGraphOpen => "feature.operation-graph-open",
            Self::FeatureThreadDefinitionPartial => "feature.thread-definition-partial",
            Self::FeatureThickenOperandsNative => "feature.thicken-operands-native",
            Self::FeatureShellDefinitionPartial => "feature.shell-definition-partial",
            Self::FeatureSheetMetalAttachedPlateKindMissing => {
                "feature.sheet-metal-attached-plate-kind-missing"
            }
            Self::FeatureSheetMetalBendKindMissing => "feature.sheet-metal-bend-kind-missing",
            Self::FeatureStateUnresolved => "feature.state-unresolved",
            Self::ParameterGraphOpen => "parameter.graph-open",
            Self::ParameterExpressionBounded => "parameter.expression-bounded",
            Self::ParameterScopeUnresolved => "parameter.scope-unresolved",
            Self::SketchGraphOpen => "sketch.graph-open",
            Self::RseStreamUnpaired => "rse.stream-unpaired",
            Self::PropertySetStreamMalformed => "property-set.stream-malformed",
            Self::MetadataPropertyUnmapped => "metadata.property-unmapped",
            Self::ProteinCatalogUndecodable => "protein.catalog-undecodable",
            Self::ProteinAssetRejected => "protein.asset-rejected",
            Self::ProteinAppearanceAbsent => "protein.appearance-absent",
            Self::AppearanceDefaultUnresolved => "appearance.default-unresolved",
            Self::ProteinGuidAmbiguous => "protein.guid-ambiguous",
            Self::MaterialDistanceUnitUntyped => "material.distance-unit-untyped",
            Self::ProteinStreamMalformed => "protein.stream-malformed",
            Self::AppearanceFaceOverrideUnresolved => "appearance.face-override-unresolved",
            Self::UfrxTableMalformed => "ufrx.table-malformed",
            Self::UfrxSchemaUnsupportedAssembly => "ufrx.schema-unsupported-assembly",
            Self::UfrxSchemaUnsupported => "ufrx.schema-unsupported",
            Self::AssemblyComponentExternal => "assembly.component-external",
            Self::AssemblyPlacementNotTransferred => "assembly.placement-not-transferred",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::KernelCarrierUnparseable => "source.kernel-carrier-unparseable",
            Self::KernelHeaderMetadataUnresolved => "source.kernel-header-metadata-unresolved",
            Self::KernelHeaderToleranceUnresolved => "source.kernel-header-tolerance-unresolved",
            Self::ContainerNoncanonical => "container.noncanonical",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::GeometryKernelCarrierNotTransferred => Severity::Blocking,
            _ => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::GeometryKernelCarrierNotTransferred
            | Self::GeometryProceduralSurfaceNotTransferred => LossTaxonomy::GeometryNotTransferred,
            Self::RseSegmentPairUntyped | Self::UfrxSchemaUnsupported => {
                LossTaxonomy::RecordNotTyped
            }
            Self::RseMetadataStreamMalformed
            | Self::RseBulkStreamMalformed
            | Self::AssemblyRecordMalformed
            | Self::PresentationRecordMalformed
            | Self::DesignRecordMalformed
            | Self::SketchRecordMalformed
            | Self::FeatureRecordMalformed
            | Self::RseStreamUnpaired
            | Self::PropertySetStreamMalformed
            | Self::ProteinStreamMalformed
            | Self::UfrxTableMalformed => LossTaxonomy::DecodeDiagnostic,
            Self::KernelCarrierUnparseable => LossTaxonomy::RecordNotTyped,
            Self::KernelHeaderMetadataUnresolved | Self::ContainerNoncanonical => {
                LossTaxonomy::NoncanonicalSourceSyntax
            }
            Self::KernelHeaderToleranceUnresolved => LossTaxonomy::GeometryNotTransferred,
            Self::FeatureOperationGraphOpen
            | Self::FeatureThreadDefinitionPartial
            | Self::FeatureThickenOperandsNative
            | Self::FeatureShellDefinitionPartial
            | Self::FeatureSheetMetalAttachedPlateKindMissing
            | Self::FeatureSheetMetalBendKindMissing
            | Self::FeatureProfileBodyUndecoded
            | Self::SketchConstructionUnresolved
            | Self::SketchPairedAssociationUnprojected
            | Self::FeatureStateUnresolved
            | Self::SketchGraphOpen => LossTaxonomy::FeatureHistoryRetained,
            Self::ParameterGraphOpen
            | Self::ParameterExpressionBounded
            | Self::ParameterScopeUnresolved => LossTaxonomy::ParametricRecordOmitted,
            Self::MetadataPropertyUnmapped
            | Self::FeatureLabelUnusable
            | Self::FeatureLabelNameReplaced => LossTaxonomy::MetadataNotTransferred,
            Self::ProteinCatalogUndecodable
            | Self::ProteinAssetRejected
            | Self::ProteinAppearanceAbsent
            | Self::AppearanceDefaultUnresolved
            | Self::ProteinGuidAmbiguous
            | Self::MaterialDistanceUnitUntyped
            | Self::AppearanceFaceOverrideUnresolved => LossTaxonomy::MaterialNotTransferred,
            Self::UfrxSchemaUnsupportedAssembly | Self::AssemblyComponentExternal => {
                LossTaxonomy::AssemblyComponentsExternal
            }
            Self::AssemblyPlacementNotTransferred => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::SourceDialectUnverified | Self::KernelDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(
            const {
                match cadmpeg_ir::report::loss::LossNamespace::new("inventor") {
                    Ok(namespace) => namespace,
                    Err(_) => panic!("reserved codec namespace"),
                }
            },
            self.code(),
            self.shared_taxonomy(),
        )
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `inventor/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::InventorLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = InventorLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "geometry.kernel-carrier-not-transferred",
                "geometry.procedural-surface-not-transferred",
                "rse.segment-pair-untyped",
                "rse.metadata-stream-malformed",
                "rse.bulk-stream-malformed",
                "assembly.record-malformed",
                "presentation.record-malformed",
                "design.record-malformed",
                "sketch.record-malformed",
                "feature.record-malformed",
                "feature.label-unusable",
                "sketch.construction-unresolved",
                "sketch.paired-association-unprojected",
                "feature.profile-body-undecoded",
                "feature.label-name-replaced",
                "feature.operation-graph-open",
                "feature.thread-definition-partial",
                "feature.thicken-operands-native",
                "feature.shell-definition-partial",
                "feature.sheet-metal-attached-plate-kind-missing",
                "feature.sheet-metal-bend-kind-missing",
                "feature.state-unresolved",
                "parameter.graph-open",
                "parameter.expression-bounded",
                "parameter.scope-unresolved",
                "sketch.graph-open",
                "rse.stream-unpaired",
                "property-set.stream-malformed",
                "metadata.property-unmapped",
                "protein.catalog-undecodable",
                "protein.asset-rejected",
                "protein.appearance-absent",
                "appearance.default-unresolved",
                "protein.guid-ambiguous",
                "material.distance-unit-untyped",
                "protein.stream-malformed",
                "appearance.face-override-unresolved",
                "ufrx.table-malformed",
                "ufrx.schema-unsupported-assembly",
                "ufrx.schema-unsupported",
                "assembly.component-external",
                "assembly.placement-not-transferred",
                "source.dialect-unverified",
                "source.kernel-dialect-unverified",
                "source.kernel-carrier-unparseable",
                "source.kernel-header-metadata-unresolved",
                "source.kernel-header-tolerance-unresolved",
                "container.noncanonical",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in InventorLossCode::ALL {
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
        for code in InventorLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert_eq!(note.code.namespace(), "inventor");
            assert_eq!(note.code.local_code(), code.code());
            assert!(note.provenance.is_none());
        }
    }
}
