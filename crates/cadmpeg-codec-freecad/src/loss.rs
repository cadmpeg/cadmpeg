// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `FreeCAD` `.fcstd` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a
//! stable machine-readable code from [`FreecadLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`FreecadLossCode::note`] is the single practical construction path for a
//! [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller.
//!
use cadmpeg_ir::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

pub(crate) mod payloads;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("fcstd");

/// A stable, machine-readable identifier for one `.fcstd` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`FreecadLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum FreecadLossCode {
    /// A formula identifier has no unique parameter binding.
    ExpressionReferenceUnresolved,
    /// A property has no constructible typed attribute projection.
    AttributePropertyUnprojected,
    ExportAttributeNotTransferred,
    /// Model payload content survives only in opaque source bytes.
    SourceModelPayloadUnrepresented,
    /// Design payload content survives only in opaque source bytes.
    SourceDesignPayloadUnrepresented,
    /// A whole neutral arena has no `FCStd` representation.
    ExportArenaNotTransferred,
    /// A neutral construction has no writable `FreeCAD` operation.
    ExportFeatureNotTransferred,
    /// An occurrence has no source placement to write.
    ExportOccurrenceNotTransferred,
    /// A joint operand occurrence has no writable placement.
    ExportJointOperandNotTransferred,
    /// A neutral body has an unsupported exact carrier.
    ExportGeometryNotTransferred,
    /// An appearance member has no exact `FreeCAD` GUI encoding.
    ExportAppearanceNotTransferred,
    /// GUI carrier requirements or aliases alter neutral presentation state.
    ExportPresentationNotTransferred,
    /// A procedural carrier is written through its current solved cache.
    ExportProceduralReduced,
    /// Native-only property content has no synthesized encoding.
    ExportNativePropertyNotTransferred,
    /// Template geometry or positioned text remains in its SVG asset.
    TemplateNotTransferred,
    /// The stored horizontal view axis has no nonzero neutral direction.
    DrawingViewAxisNotTransferred,
    /// Projected model-view edges have no persisted curve collection.
    ViewGeometryNotStored,
    /// The view family has no admitted projected geometry.
    ViewGeometryNotTransferred,
    /// An exact dimension value has no admitted page reference anchors.
    DimensionAnchorsNotTransferred,
    /// Drawing appearance has no complete physical style.
    DrawingStyleNotTransferred,
    /// Caption text has no normalized page position.
    CaptionNotTransferred,
    /// Persisted drawing content remains native.
    DrawingSheetNotTransferred,
    /// Persisted drawing content remains native.
    DrawingTitleBlockNotTransferred,
    /// Persisted drawing content remains native.
    DrawingCosmeticEdgeNotTransferred,
    /// Persisted drawing content remains native.
    DrawingCosmeticVertexNotTransferred,
    /// Persisted drawing content remains native.
    DrawingCenterLineNotTransferred,
    /// Persisted drawing content remains native.
    DrawingGeometryFormatNotTransferred,
    /// Persisted drawing content remains native.
    DrawingDimensionKindNotTransferred,
    /// Persisted drawing content remains native.
    DrawingDimensionGeometryNotTransferred,
    /// Persisted drawing content remains native.
    DrawingDimensionToleranceNotTransferred,
    /// A mesh use has no complete neutral placement path.
    MeshPlacementNotTransferred,
    /// Mesh occurrence expansion reached its input-scaled local work cap.
    MeshPlacementBounded,
    /// An optional file reference has no readable archive payload.
    PersistenceSideEntryUnresolved,
    /// Redundant persistence counts disagree with framed XML populations.
    PersistenceCountNoncanonical,
    /// Feature history cannot enter a neutral definition because its ordering is cyclic.
    FeatureCyclicHistory,
    /// Feature retains its native kind without a complete neutral operation.
    FeatureNativeKindRetained,
    /// A revolution has no complete admitted angular construction.
    FeatureRevolutionNotTransferred,
    /// A primitive has no admitted local-to-model frame.
    FeaturePrimitivePlacementUnresolved,
    /// A blind extrusion length has no evaluated value; the feature is
    /// written without Length (§7.11.2).
    FeatureLengthNotTransferred,
    /// Sketch geometry record retains a native kind without solved geometry.
    SketchNativeGeometry,
    /// A sketch B-spline has no admitted finite neutral representation.
    SketchNurbsNative,
    /// Cached sketch geometry has no matching live external reference.
    SketchExternalReferenceUnresolved,
    /// Optional external sketch cache cannot be projected; live links remain authoritative.
    SketchExternalCacheUnresolved,
    /// Sketch constraint retains a native relation kind without neutral semantics.
    SketchNativeConstraint,
    /// More than one sketch constraint has the same nonempty name.
    SketchConstraintNameAmbiguous,
    /// A sketch constraint has both index and name expression bindings.
    SketchConstraintBindingConflict,
    /// Sketch profiles are not chained because endpoint matching reached its work cap.
    SketchProfileMatchingBounded,
    /// Topology color values were retained because their count did not match mapped topology.
    AppearanceTopologyColorCountMismatch,
    /// A primitive style size could not enter the neutral appearance.
    AppearancePrimitiveSizeNotTransferred,
    /// The declared persistence schema names no dialect this codec has a strategy for.
    SourceDialectUnverified,
    /// Synthesis writes a different dialect from the current source identity.
    SourceDialectDisplaced,
    /// The GUI document used schema-1 vocabulary under another declaration.
    SourceGuiSchemaUnverified,
    /// A GUI property states a blank key, so its value has no name to carry.
    SourceGuiPropertyKeyBlank,
    /// Optional GUI records could not be interpreted; exact XML remains retained.
    SourceGuiMetadataUnresolved,
    /// Optional persistent naming metadata could not be interpreted.
    ElementMapMetadataUnresolved,
    /// A parameter-space curve could not enter neutral geometry.
    PcurveNotTransferred,
    /// Equal-matrix pcurves have no admitted matching native location chain.
    PcurveLocationAmbiguous,
    /// Native topology remains retained when location identity expansion reaches its local cap.
    TopologyLocationIdentityBounded,
    /// Producer version metadata used an alias or conflicting declarations.
    ProgramVersionNoncanonical,
    /// A feature type name does not name an encodable definition.
    FeatureSourceTagNotTransferred,
    /// Bodies sharing one object disagree on independent display style.
    AppearanceBodyStyleNotTransferred,
    /// A body name has no native home on its emitted object.
    BodyNameNotTransferred,
    /// A product label differs from the feature label sharing its object.
    ProductLabelNotTransferred,
    /// A product source name is not a valid free identifier.
    ProductSourceNameNotTransferred,
    /// Linked prototype placements disagree, so the effective placement is baked.
    ProductLinkedPlacementFolded,
    /// A parameter name is not a valid selector or native alias.
    ParameterNameNotTransferred,
    /// A parameter has no driving flag in its native home.
    ParameterDrivingNotTransferred,
    /// Persistence version metadata used a lowercase alias.
    FileVersionNoncanonical,
    /// Persistence file-version declaration cannot select a legacy map encoding.
    FileVersionUnverified,
    /// Nonempty inline legacy map has no verified file-version interpretation.
    ElementMapVersionUnresolved,
}

impl FreecadLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [FreecadLossCode] = &[
        Self::ExpressionReferenceUnresolved,
        Self::AttributePropertyUnprojected,
        Self::ExportAttributeNotTransferred,
        Self::SourceModelPayloadUnrepresented,
        Self::SourceDesignPayloadUnrepresented,
        Self::ExportArenaNotTransferred,
        Self::ExportFeatureNotTransferred,
        Self::ExportOccurrenceNotTransferred,
        Self::ExportJointOperandNotTransferred,
        Self::ExportGeometryNotTransferred,
        Self::ExportAppearanceNotTransferred,
        Self::ExportPresentationNotTransferred,
        Self::ExportProceduralReduced,
        Self::ExportNativePropertyNotTransferred,
        Self::TemplateNotTransferred,
        Self::DrawingViewAxisNotTransferred,
        Self::ViewGeometryNotStored,
        Self::ViewGeometryNotTransferred,
        Self::DimensionAnchorsNotTransferred,
        Self::DrawingStyleNotTransferred,
        Self::CaptionNotTransferred,
        Self::DrawingSheetNotTransferred,
        Self::DrawingTitleBlockNotTransferred,
        Self::DrawingCosmeticEdgeNotTransferred,
        Self::DrawingCosmeticVertexNotTransferred,
        Self::DrawingCenterLineNotTransferred,
        Self::DrawingGeometryFormatNotTransferred,
        Self::DrawingDimensionKindNotTransferred,
        Self::DrawingDimensionGeometryNotTransferred,
        Self::DrawingDimensionToleranceNotTransferred,
        Self::MeshPlacementNotTransferred,
        Self::MeshPlacementBounded,
        Self::PersistenceSideEntryUnresolved,
        Self::PersistenceCountNoncanonical,
        Self::FeatureCyclicHistory,
        Self::FeatureNativeKindRetained,
        Self::FeatureRevolutionNotTransferred,
        Self::FeaturePrimitivePlacementUnresolved,
        Self::FeatureLengthNotTransferred,
        Self::SketchNativeGeometry,
        Self::SketchNurbsNative,
        Self::SketchNativeConstraint,
        Self::SketchConstraintNameAmbiguous,
        Self::SketchConstraintBindingConflict,
        Self::SketchExternalReferenceUnresolved,
        Self::SketchExternalCacheUnresolved,
        Self::SketchProfileMatchingBounded,
        Self::AppearanceTopologyColorCountMismatch,
        Self::AppearancePrimitiveSizeNotTransferred,
        Self::SourceDialectUnverified,
        Self::SourceDialectDisplaced,
        Self::SourceGuiSchemaUnverified,
        Self::SourceGuiPropertyKeyBlank,
        Self::SourceGuiMetadataUnresolved,
        Self::ElementMapMetadataUnresolved,
        Self::PcurveNotTransferred,
        Self::PcurveLocationAmbiguous,
        Self::TopologyLocationIdentityBounded,
        Self::ProgramVersionNoncanonical,
        Self::FeatureSourceTagNotTransferred,
        Self::AppearanceBodyStyleNotTransferred,
        Self::BodyNameNotTransferred,
        Self::ProductLabelNotTransferred,
        Self::ProductSourceNameNotTransferred,
        Self::ProductLinkedPlacementFolded,
        Self::ParameterNameNotTransferred,
        Self::ParameterDrivingNotTransferred,
        Self::FileVersionNoncanonical,
        Self::FileVersionUnverified,
        Self::ElementMapVersionUnresolved,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::ExpressionReferenceUnresolved => "expression.reference-unresolved",
            Self::AttributePropertyUnprojected => "attribute.property-unprojected",
            Self::ExportAttributeNotTransferred => "attribute.not-transferred",
            Self::SourceModelPayloadUnrepresented => "source.model-payload-unrepresented",
            Self::SourceDesignPayloadUnrepresented => "source.design-payload-unrepresented",
            Self::ExportArenaNotTransferred => "arena.not-transferred",
            Self::ExportFeatureNotTransferred => "feature.not-transferred",
            Self::ExportOccurrenceNotTransferred => "occurrence.not-transferred",
            Self::ExportJointOperandNotTransferred => "joint.operand-not-transferred",
            Self::ExportGeometryNotTransferred => "geometry.not-transferred",
            Self::ExportAppearanceNotTransferred => "appearance.not-transferred",
            Self::ExportPresentationNotTransferred => "presentation.not-transferred",
            Self::ExportProceduralReduced => "geometry.procedural-reduced",
            Self::ExportNativePropertyNotTransferred => "metadata.native-property-not-transferred",
            Self::DrawingViewAxisNotTransferred => "drawing.view-axis-not-transferred",
            Self::TemplateNotTransferred => "drawing.template-not-transferred",
            Self::ViewGeometryNotStored => "drawing.view-geometry-not-stored",
            Self::ViewGeometryNotTransferred => "drawing.view-geometry-not-transferred",
            Self::DimensionAnchorsNotTransferred => "annotation.dimension-anchors-not-transferred",
            Self::DrawingStyleNotTransferred => "drawing.style-not-transferred",
            Self::CaptionNotTransferred => "drawing.caption-not-transferred",
            Self::DrawingSheetNotTransferred => "drawing.sheet-not-transferred",
            Self::DrawingTitleBlockNotTransferred => "drawing.title-block-not-transferred",
            Self::DrawingCosmeticEdgeNotTransferred => "drawing.cosmetic-edge-not-transferred",
            Self::DrawingCosmeticVertexNotTransferred => "drawing.cosmetic-vertex-not-transferred",
            Self::DrawingCenterLineNotTransferred => "drawing.center-line-not-transferred",
            Self::DrawingGeometryFormatNotTransferred => "drawing.geometry-format-not-transferred",
            Self::DrawingDimensionKindNotTransferred => "drawing.dimension-kind-not-transferred",
            Self::DrawingDimensionGeometryNotTransferred => {
                "drawing.dimension-geometry-not-transferred"
            }
            Self::DrawingDimensionToleranceNotTransferred => {
                "drawing.dimension-tolerance-not-transferred"
            }
            Self::MeshPlacementNotTransferred => "geometry.mesh-placement-not-transferred",
            Self::MeshPlacementBounded => "geometry.mesh-placement-bounded",
            Self::PersistenceSideEntryUnresolved => "metadata.persistence-side-entry-unresolved",
            Self::PersistenceCountNoncanonical => "metadata.persistence-count-noncanonical",
            Self::FeatureCyclicHistory => "feature.cyclic-history",
            Self::FeaturePrimitivePlacementUnresolved => "feature.primitive-placement-unresolved",
            Self::FeatureLengthNotTransferred => "feature.length-not-transferred",
            Self::FeatureNativeKindRetained => "feature.native-kind-retained",
            Self::FeatureRevolutionNotTransferred => "feature.revolution-not-transferred",
            Self::SketchNativeGeometry => "sketch.native-geometry",
            Self::SketchNurbsNative => "sketch.nurbs-native",
            Self::SketchNativeConstraint => "sketch.native-constraint",
            Self::SketchConstraintNameAmbiguous => "sketch.constraint-name-ambiguous",
            Self::SketchConstraintBindingConflict => "sketch.constraint-binding-conflict",
            Self::SketchExternalReferenceUnresolved => "sketch.external-reference-unresolved",
            Self::SketchExternalCacheUnresolved => "sketch.external-cache-unresolved",
            Self::SketchProfileMatchingBounded => "sketch.profile-matching-bounded",
            Self::AppearanceTopologyColorCountMismatch => {
                "appearance.topology-color-count-mismatch"
            }
            Self::AppearancePrimitiveSizeNotTransferred => {
                "appearance.primitive-size-not-transferred"
            }
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
            Self::SourceGuiSchemaUnverified => "source.gui-schema-unverified",
            Self::SourceGuiPropertyKeyBlank => "source.gui-property-key-blank",
            Self::SourceGuiMetadataUnresolved => "source.gui-metadata-unresolved",
            Self::ElementMapMetadataUnresolved => "element-map.metadata-unresolved",
            Self::PcurveNotTransferred => "pcurve.not-transferred",
            Self::PcurveLocationAmbiguous => "pcurve.location-ambiguous",
            Self::TopologyLocationIdentityBounded => "topology.location-identity-bounded",
            Self::ProgramVersionNoncanonical => "metadata.program-version-noncanonical",
            Self::FeatureSourceTagNotTransferred => "feature.source-tag-not-transferred",
            Self::AppearanceBodyStyleNotTransferred => "appearance.body-style-not-transferred",
            Self::BodyNameNotTransferred => "body.name-not-transferred",
            Self::ProductLabelNotTransferred => "product.label-not-transferred",
            Self::ProductSourceNameNotTransferred => "product.source-name-not-transferred",
            Self::ProductLinkedPlacementFolded => "product.linked-placement-folded",
            Self::ParameterNameNotTransferred => "parameter.name-not-transferred",
            Self::ParameterDrivingNotTransferred => "parameter.driving-not-transferred",
            Self::FileVersionNoncanonical => "metadata.file-version-noncanonical",
            Self::FileVersionUnverified => "metadata.file-version-unverified",
            Self::ElementMapVersionUnresolved => "element-map.version-unresolved",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::ExpressionReferenceUnresolved
            | Self::AttributePropertyUnprojected
            | Self::ExportAttributeNotTransferred
            | Self::ExportOccurrenceNotTransferred
            | Self::ExportJointOperandNotTransferred => Severity::Warning,
            Self::ExportFeatureNotTransferred | Self::ExportGeometryNotTransferred => {
                Severity::Error
            }
            Self::ExportNativePropertyNotTransferred => Severity::Info,
            Self::FeatureSourceTagNotTransferred
            | Self::BodyNameNotTransferred
            | Self::ProductLabelNotTransferred
            | Self::ProductSourceNameNotTransferred => Severity::Info,
            Self::SourceModelPayloadUnrepresented
            | Self::SourceDesignPayloadUnrepresented
            | Self::ExportArenaNotTransferred
            | Self::ExportAppearanceNotTransferred
            | Self::ExportPresentationNotTransferred
            | Self::ExportProceduralReduced => Severity::Warning,
            Self::MeshPlacementNotTransferred
            | Self::MeshPlacementBounded
            | Self::FeatureNativeKindRetained
            | Self::FeatureRevolutionNotTransferred
            | Self::FeaturePrimitivePlacementUnresolved
            | Self::SketchNativeGeometry
            | Self::SketchNurbsNative
            | Self::SketchNativeConstraint
            | Self::SketchConstraintNameAmbiguous
            | Self::SketchConstraintBindingConflict
            | Self::PcurveLocationAmbiguous
            | Self::TopologyLocationIdentityBounded => Severity::Blocking,
            Self::FeatureCyclicHistory
            | Self::DrawingViewAxisNotTransferred
            | Self::TemplateNotTransferred
            | Self::ViewGeometryNotStored
            | Self::ViewGeometryNotTransferred
            | Self::DimensionAnchorsNotTransferred
            | Self::DrawingStyleNotTransferred
            | Self::CaptionNotTransferred
            | Self::DrawingSheetNotTransferred
            | Self::DrawingTitleBlockNotTransferred
            | Self::DrawingCosmeticEdgeNotTransferred
            | Self::DrawingCosmeticVertexNotTransferred
            | Self::DrawingCenterLineNotTransferred
            | Self::DrawingGeometryFormatNotTransferred
            | Self::DrawingDimensionKindNotTransferred
            | Self::DrawingDimensionGeometryNotTransferred
            | Self::DrawingDimensionToleranceNotTransferred
            | Self::SketchExternalCacheUnresolved
            | Self::SketchExternalReferenceUnresolved
            | Self::SketchProfileMatchingBounded
            | Self::PersistenceSideEntryUnresolved
            | Self::PersistenceCountNoncanonical
            | Self::AppearanceTopologyColorCountMismatch
            | Self::AppearancePrimitiveSizeNotTransferred
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::SourceGuiSchemaUnverified
            | Self::SourceGuiPropertyKeyBlank
            | Self::SourceGuiMetadataUnresolved
            | Self::ElementMapMetadataUnresolved
            | Self::PcurveNotTransferred
            | Self::ProgramVersionNoncanonical
            | Self::AppearanceBodyStyleNotTransferred
            | Self::ProductLinkedPlacementFolded
            | Self::ParameterNameNotTransferred
            | Self::ParameterDrivingNotTransferred
            | Self::FileVersionNoncanonical
            | Self::FileVersionUnverified
            | Self::ElementMapVersionUnresolved
            | Self::FeatureLengthNotTransferred => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ExportOccurrenceNotTransferred | Self::ExportJointOperandNotTransferred => {
                LossTaxonomy::AssemblyPlacementsNotTransferred
            }
            Self::ExpressionReferenceUnresolved
            | Self::AttributePropertyUnprojected
            | Self::ExportAttributeNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::SourceModelPayloadUnrepresented | Self::SourceDesignPayloadUnrepresented => {
                LossTaxonomy::RecordNotTyped
            }
            Self::ExportArenaNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::ExportFeatureNotTransferred => LossTaxonomy::ParametricRecordOmitted,
            Self::ExportAppearanceNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::ExportPresentationNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::ExportProceduralReduced => LossTaxonomy::ProceduralReduced,
            Self::ExportGeometryNotTransferred => LossTaxonomy::GeometryNotTransferred,
            Self::ExportNativePropertyNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::MeshPlacementNotTransferred | Self::MeshPlacementBounded => {
                LossTaxonomy::GeometryNotTransferred
            }
            Self::DrawingViewAxisNotTransferred
            | Self::TemplateNotTransferred
            | Self::ViewGeometryNotStored
            | Self::ViewGeometryNotTransferred
            | Self::DimensionAnchorsNotTransferred
            | Self::DrawingStyleNotTransferred
            | Self::CaptionNotTransferred
            | Self::DrawingSheetNotTransferred
            | Self::DrawingTitleBlockNotTransferred
            | Self::DrawingCosmeticEdgeNotTransferred
            | Self::DrawingCosmeticVertexNotTransferred
            | Self::DrawingCenterLineNotTransferred
            | Self::DrawingGeometryFormatNotTransferred
            | Self::DrawingDimensionKindNotTransferred
            | Self::DrawingDimensionGeometryNotTransferred
            | Self::DrawingDimensionToleranceNotTransferred
            | Self::PersistenceSideEntryUnresolved => LossTaxonomy::RecordNotTyped,
            Self::FeatureCyclicHistory
            | Self::FeatureNativeKindRetained
            | Self::FeatureRevolutionNotTransferred
            | Self::FeaturePrimitivePlacementUnresolved
            | Self::FeatureSourceTagNotTransferred => LossTaxonomy::FeatureHistoryRetained,
            Self::SketchExternalCacheUnresolved
            | Self::SketchExternalReferenceUnresolved
            | Self::SketchNativeGeometry
            | Self::SketchNurbsNative
            | Self::SketchNativeConstraint
            | Self::SketchConstraintNameAmbiguous
            | Self::SketchConstraintBindingConflict
            | Self::SketchProfileMatchingBounded
            | Self::SourceGuiPropertyKeyBlank
            | Self::SourceGuiMetadataUnresolved
            | Self::ElementMapMetadataUnresolved
            | Self::PcurveNotTransferred
            | Self::PcurveLocationAmbiguous
            | Self::TopologyLocationIdentityBounded => LossTaxonomy::RecordNotTyped,
            Self::AppearanceTopologyColorCountMismatch
            | Self::AppearancePrimitiveSizeNotTransferred
            | Self::AppearanceBodyStyleNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::BodyNameNotTransferred
            | Self::ProductLabelNotTransferred
            | Self::ProductSourceNameNotTransferred
            | Self::ProductLinkedPlacementFolded
            | Self::ParameterNameNotTransferred
            | Self::FeatureLengthNotTransferred
            | Self::ParameterDrivingNotTransferred => LossTaxonomy::MetadataNotTransferred,
            Self::PersistenceCountNoncanonical
            | Self::ProgramVersionNoncanonical
            | Self::FileVersionNoncanonical
            | Self::FileVersionUnverified => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ElementMapVersionUnresolved => LossTaxonomy::RecordNotTyped,
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::SourceGuiSchemaUnverified => LossTaxonomy::SourceDialectUnverified,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    fn kind(self) -> LossKind {
        let strict_floor = match self {
            Self::SourceGuiSchemaUnverified => None,
            other => other.shared_taxonomy().strict_floor(),
        };
        NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy())
            .with_strict_floor(strict_floor)
            .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `fcstd/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

/// A value attribute that names a sidecar entry: a file payload reference,
/// not an external document link or a File-family record name.
pub(crate) fn entry_attribute(property: &crate::native::PropertyRecord, name: &str) -> bool {
    (matches!(name, "file" | "File") && !crate::persistence::is_xlink_type(&property.type_name))
        || (property.family == crate::native::PropertyFamily::File
            && matches!(name, "name" | "Name"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::FreecadLossCode;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = FreecadLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "expression.reference-unresolved",
                "attribute.property-unprojected",
                "attribute.not-transferred",
                "source.model-payload-unrepresented",
                "source.design-payload-unrepresented",
                "arena.not-transferred",
                "feature.not-transferred",
                "occurrence.not-transferred",
                "joint.operand-not-transferred",
                "geometry.not-transferred",
                "appearance.not-transferred",
                "presentation.not-transferred",
                "geometry.procedural-reduced",
                "metadata.native-property-not-transferred",
                "drawing.template-not-transferred",
                "drawing.view-axis-not-transferred",
                "drawing.view-geometry-not-stored",
                "drawing.view-geometry-not-transferred",
                "annotation.dimension-anchors-not-transferred",
                "drawing.style-not-transferred",
                "drawing.caption-not-transferred",
                "drawing.sheet-not-transferred",
                "drawing.title-block-not-transferred",
                "drawing.cosmetic-edge-not-transferred",
                "drawing.cosmetic-vertex-not-transferred",
                "drawing.center-line-not-transferred",
                "drawing.geometry-format-not-transferred",
                "drawing.dimension-kind-not-transferred",
                "drawing.dimension-geometry-not-transferred",
                "drawing.dimension-tolerance-not-transferred",
                "geometry.mesh-placement-not-transferred",
                "geometry.mesh-placement-bounded",
                "metadata.persistence-side-entry-unresolved",
                "metadata.persistence-count-noncanonical",
                "feature.cyclic-history",
                "feature.native-kind-retained",
                "feature.revolution-not-transferred",
                "feature.primitive-placement-unresolved",
                "feature.length-not-transferred",
                "sketch.native-geometry",
                "sketch.nurbs-native",
                "sketch.native-constraint",
                "sketch.constraint-name-ambiguous",
                "sketch.constraint-binding-conflict",
                "sketch.external-reference-unresolved",
                "sketch.external-cache-unresolved",
                "sketch.profile-matching-bounded",
                "appearance.topology-color-count-mismatch",
                "appearance.primitive-size-not-transferred",
                "source.dialect-unverified",
                "target.source-dialect-displaced",
                "source.gui-schema-unverified",
                "source.gui-property-key-blank",
                "source.gui-metadata-unresolved",
                "element-map.metadata-unresolved",
                "pcurve.not-transferred",
                "pcurve.location-ambiguous",
                "topology.location-identity-bounded",
                "metadata.program-version-noncanonical",
                "feature.source-tag-not-transferred",
                "appearance.body-style-not-transferred",
                "body.name-not-transferred",
                "product.label-not-transferred",
                "product.source-name-not-transferred",
                "product.linked-placement-folded",
                "parameter.name-not-transferred",
                "parameter.driving-not-transferred",
                "metadata.file-version-noncanonical",
                "metadata.file-version-unverified",
                "element-map.version-unresolved",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in FreecadLossCode::ALL {
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
        for code in FreecadLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }

    #[test]
    fn gui_schema_recovery_does_not_trigger_document_dialect_strictness() {
        assert_eq!(
            FreecadLossCode::SourceGuiSchemaUnverified
                .kind()
                .strict_floor(),
            None
        );
        assert_eq!(
            FreecadLossCode::SourceDialectUnverified
                .kind()
                .strict_floor(),
            Some(cadmpeg_ir::report::Severity::Warning)
        );
    }

    #[test]
    fn unreadable_archive_payload_is_not_represented_by_its_typed_diagnostic() {
        use cadmpeg_ir::{Codec, DecodeOptions};
        use cadmpeg_test_support::zip_fixture::{self, RawEntry};
        let xml = r#"<Document SchemaVersion="4"><Objects Count="1"><Object name="Sketch" type="Sketcher::SketchObject"/></Objects><ObjectData Count="1"><Object name="Sketch"><Properties Count="1"><Property name="ExternalGeo" type="App::PropertyFile"><File file="cache.bin"/></Property></Properties></Object></ObjectData></Document>"#;
        for (name, expected) in [
            ("cache.bin", cadmpeg_ir::ArenaClass::Design),
            ("Shape.brp", cadmpeg_ir::ArenaClass::Model),
            ("Thumbnail.png", cadmpeg_ir::ArenaClass::Model),
        ] {
            let bytes = zip_fixture::archive(&[
                RawEntry::stored("Document.xml", xml.as_bytes()),
                RawEntry::compressed(name, 12, &[1], &[1]),
            ]);
            let decoded = crate::FcstdCodec
                .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                .unwrap();
            let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
            let [(_, retained)] = parts.as_slice() else {
                panic!("one given-up member: {parts:?}")
            };
            assert!(decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "record.unreadable"));
            assert!(!decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arenas()
                .contains_key("unreadable_entries"));
            assert_eq!(retained.data(), Some([1].as_slice()));
            assert_eq!(
                decoded.source_fidelity().unrepresented(),
                cadmpeg_ir::ArenaClasses::from([expected])
            );
        }
    }
}
