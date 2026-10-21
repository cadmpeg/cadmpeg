// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for Fusion `.f3d` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`F3dLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`F3dLossCode::note`] is the single practical construction path for a
//! [`LossNote`] in this crate: it fixes the loss category and severity from
//! the code so the two cannot drift apart across sites, and it leaves only
//! the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `f3d` namespace.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

/// A stable, machine-readable identifier for one Fusion `.f3d` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`F3dLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum F3dLossCode {
    /// Payload-bearing Design dimension companions have no typed locus frame.
    DimensionCompanionUntyped,
    /// Design configuration JSON members have no assigned neutral semantics.
    ConfigurationMemberUnassigned,
    /// An active override has no evaluated cache for its changed expression.
    ConfigurationActiveCacheUnavailable,
    /// Design configuration rules have no unambiguous activation target.
    ConfigurationRuleUnbound,
    /// Design configuration parameter overrides have no parameter identity.
    ConfigurationParameterOverrideUnbound,
    /// Design configuration feature suppressions have no feature identity.
    ConfigurationFeatureSuppressionUnbound,
    /// Non-root `ACT` component links stay source-only; product role is unresolved.
    ActComponentLinkUnresolved,
    /// ACT records whose semantic payload could not be decoded.
    ActRecordUntyped,
    /// Local component occurrences whose container or definition the source does not settle.
    LocalOccurrenceUnplaced,
    /// Unresolved `Component Insert` occurrences whose placement has no finite millimetre transform.
    ComponentInsertUnplaced,
    /// Unresolved `Component Insert` occurrences dropped because no authored timeline order tells
    /// top-level inserts from internal operands.
    ComponentInsertOrderUnknown,
    /// Design assembly joint scopes whose operands, frames, limits, or alignment values do not
    /// project.
    AssemblyJointUnprojected,
    /// An F3Z drawing root was omitted while its derived 3D model transferred.
    DrawingDocumentOmitted,
    /// ASM history estimated retained size exceeded the session's remaining retained-byte allowance.
    HistoryBindingBudgetExceeded,
    /// ASM history topology binding exceeded its local work cap.
    HistoryBindingWorkExceeded,
    /// A Design recipe side-sequence search exceeded its local work cap.
    RecipeSideSearchBudgetExceeded,
    /// A dimension recipe retains prefix grouping outside its typed references.
    RecipePrefixUntyped,
    /// Ordering one owner scope's Design parameters exceeded its local work cap.
    ParameterOrderBudgetExceeded,
    /// An ASM history span stays opaque because record framing failed.
    HistoryRecordFramingFailed,
    /// Design body-map pairs do not resolve to a body in the named BREP blob.
    DesignBodyBindingUnresolved,
    /// Reference-image timeline objects retain native Canvas records.
    ReferenceImageNativeRetained,
    /// Decal timeline objects retain native image and mapping records.
    DecalNativeRetained,
    /// Source parametric edge references marked lost have no independent proof.
    EdgeReferenceLostUnrepaired,
    /// Feature scopes have no complete neutral feature definition.
    FeatureDefinitionIncomplete,
    /// Decoded feature scopes have no neutral construction-history feature.
    FeatureScopeUnprojected,
    /// A decoded native feature envelope has no interpreted specialized payload.
    FeatureScopePayloadUninterpreted,
    /// Decoded Design parameters have no neutral parameter.
    ParameterUnprojected,
    /// Typed compact parameter frames match no parameter grammar.
    ParameterRecordUnparsed,
    /// Design parameter owner bindings have no recognized feature scope.
    ParameterOwnerUnrecognized,
    /// Design parameters retain unit tokens without a settled quantity kind.
    ParameterUnitUntyped,
    /// Material Distance properties retain unit tags without a length conversion.
    MaterialDistanceUnitUntyped,
    /// An embedded Design image remains outside the typed asset arena.
    ImageAssetUntransferred,
    /// Parameter expression symbols name same-stream parameters without an edge.
    ParameterExpressionUnbound,
    /// Feature history-state dependency links were not projected.
    HistoryDependencyUnprojected,
    /// Feature history-state dependency links have multiple source scopes.
    HistoryDependencyAmbiguous,
    /// A historical face selection cannot bind uniquely to emitted input topology members.
    HistoricalFaceSelectionUnbound,
    /// Sketch relations retain native operands; no unique neutral relation.
    SketchRelationNativeRetained,
    /// Sketch dimensions retain native operands; no unique neutral dimension.
    SketchDimensionNativeRetained,
    /// Decoded Sketch placements have no neutral sketch.
    SketchPlacementUnprojected,
    /// Decoded sketch points have no neutral sketch entity.
    SketchPointUnprojected,
    /// Decoded sketch curves have no neutral sketch entity.
    SketchCurveUnprojected,
    /// Decoded sketch surfaces have no neutral spatial sketch entity.
    SketchSurfaceUnprojected,
    /// Decoded sketch text records have no neutral sketch entity.
    SketchTextUnprojected,
    /// Decoded sketch relations have no neutral constraint.
    SketchRelationUnprojected,
    /// A spatial sketch's closed-profile search stopped at a local cap.
    SketchProfileSearchCapped,
    /// Design dimension parameters have no parameter-backed sketch constraint.
    DimensionUnprojected,
    /// Feature profile selections retain native identities; no unique profile.
    FeatureProfileSelectionNative,
    /// Extrude profile selections skipped the sketch arrangement search at a local cap.
    FeatureProfileArrangementCapped,
    /// Extrude profile selections skipped a certified profile test at a local cap.
    FeatureProfileDensificationCapped,
    /// Feature path selections retain native identities; no unique path.
    FeaturePathSelectionNative,
    /// Feature face selections retain native candidates; no unique face.
    FeatureFaceSelectionNative,
    /// Legacy face operands use a current active face after no historical slot proof.
    FeatureFaceSelectionActiveSubstituted,
    /// Feature body selections retain native identities; no unique body.
    FeatureBodySelectionNative,
    /// Feature face operands stay unresolved inside historical selections.
    FeatureFaceOperandUnresolved,
    /// Edge-treatment selections retain native recipes; no historical edge.
    FeatureEdgeSelectionNative,
    /// Edge-treatment operands stay unresolved inside historical selections.
    FeatureEdgeOperandUnresolved,
    /// Edge-treatment selections are unresolved because source edges were lost.
    FeatureEdgeSelectionLost,
    /// A rectangular-pattern instance search stopped at a local cap.
    FeaturePatternInstanceSearchCapped,
    /// Design-referenced BREP blobs could not be decoded.
    BrepBlobUndecoded,
    /// A mesh geometry container decoded without a complete Design body join.
    MeshContainerUnjoined,
    /// A mesh geometry container was not decoded.
    MeshContainerUndecoded,
    /// A Design mesh body names a container that did not uniquely join.
    MeshContainerMissing,
    /// Mesh attribute channels have no settled stored-element layout.
    MeshAttributeNotTransferred,
    /// The external-reference table was present but not decoded.
    XrefTableUndecoded,
    /// A typed occurrence placement named a role but failed its payload grammar.
    XrefPlacementUndecoded,
    /// Structured placement records were superseded by scope-bound carriers.
    XrefPlacementSuperseded,
    /// Mesh body geometry stores vertex coordinates at f32 precision.
    MeshVertexPrecisionReduced,
    /// A bodyless design's sketches or reference images are its complete carrier.
    BodylessDesignCarrier,
    /// An assembly document's geometry lives in external references.
    AssemblyComponentsExternal,
    /// Spline surface records were decoded into NURBS carriers from cache.
    NurbsSurfaceCarrier,
    /// Procedural curve records were decoded into NURBS carriers from cache.
    NurbsCurveCarrier,
    /// A stored solved surface cache contradicts its independently evaluated construction.
    SurfaceCacheRejected,
    /// Faces were omitted because their required surface reference was dangling.
    FaceSurfaceReferenceDangling,
    /// Faces rest on spline or procedural surfaces whose shape was not decoded.
    SurfaceShapeNotDecoded,
    /// Faces use zero-payload `mesh_surface` sentinels; exact surfaces are absent.
    MeshSurfaceSentinel,
    /// Edges reference a procedural 3D curve with no decodable B-spline cache.
    ProceduralCurveUndecoded,
    /// Coedges carry a UV pcurve reference with no decodable 2D carrier.
    PcurveUndecoded,
    /// Rolling-ball blend definitions resolved only one of two native supports.
    BlendSupportPartial,
    /// Solved-record application or refinement records were not transferred.
    SolvedRecordUntyped,
    /// Materials and appearances were not transferred.
    MaterialNotTransferred,
    /// ASM BREP geometry was not transferred from the container.
    GeometryNotTransferred,
    /// B-rep topology graph was not built from the container.
    TopologyNotTransferred,
    /// No ASM BREP stream was found or selected as the document geometry stream.
    MissingGeometryStream,
    /// An `XREF` path cycles through a member already on the resolution stack.
    XrefCycle,
    /// An `XREF` member is not present in the archive.
    XrefMemberMissing,
    /// An `XREF` member failed to decode.
    XrefMemberUndecoded,
    /// T-spline records were retained without typed semantics.
    TsplineRecordUntyped,
    /// A T-spline control cage was not decoded.
    TsplineCageUndecoded,
    /// Preserved source image required for a byte-exact write was unavailable.
    SourcePreservedImageUnavailable,
    /// The document was read with a grammar its own declarations do not select.
    SourceDialectUnverified,
    /// Export omits records named by the counted loss message.
    WriterAppearancePropertiesOmitted,
    /// The selected write target differs from the same-format source dialect.
    SourceDialectDisplaced,
    /// An embedded kernel carrier was read with an unverified ACIS grammar.
    KernelDialectUnverified,
    /// An embedded kernel carrier could not be framed for dialect inspection.
    KernelCarrierUnparseable,
    /// Two F3Z member layers resolved to the same core dialect identity.
    DialectLayerCollision,
    /// A framed text B-rep carries malformed header metadata or tolerances.
    TextHeaderNoncanonical,
    /// An optional kernel tolerance cannot enter the canonical document units.
    KernelHeaderToleranceUnresolved,
    /// An independently framed binary kernel product string is unreadable.
    KernelHeaderMetadataUnresolved,
    /// A bounded descriptive manifest payload is unreadable.
    ManifestMetadataUnresolved,
    /// An optional bounded archive payload is unreadable.
    ArchiveEntryUnreadable,
    /// Typed native content without a supported synthesized record form.
    WriterNativeRecordsOmitted,
    /// Content that the synthesized writer cannot represent.
    WriterDesignRecordsOmitted,
    /// Content that the synthesized writer cannot represent.
    WriterGeometryOmitted,
    /// Content that the synthesized writer cannot represent.
    WriterAuxiliaryOmitted,
    /// Content that the synthesized writer cannot represent.
    WriterProceduralReduced,
    /// Content that the synthesized writer cannot represent.
    WriterNoExportableSolids,
    /// Content outside the synthesized F3D encoding profile.
    WriterSubdOmitted,
    /// Content outside the synthesized F3D encoding profile.
    WriterAssetOmitted,
    /// Content outside the synthesized F3D encoding profile.
    WriterTessellationOmitted,
    /// Content outside the synthesized F3D encoding profile.
    WriterPcurveOmitted,
    /// Content outside the synthesized F3D encoding profile.
    WriterPmiOmitted,
    /// Content outside the synthesized F3D encoding profile.
    WriterProductOmitted,
    /// Content outside the synthesized F3D encoding profile.
    WriterSourceAssociationOmitted,

    /// Design-intent decoding failed; its sketches, features and history were dropped.
    DesignIntentUndecoded,
}

impl F3dLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [F3dLossCode] = &[
        Self::DimensionCompanionUntyped,
        Self::ConfigurationMemberUnassigned,
        Self::ConfigurationActiveCacheUnavailable,
        Self::ConfigurationRuleUnbound,
        Self::ConfigurationParameterOverrideUnbound,
        Self::ConfigurationFeatureSuppressionUnbound,
        Self::ActComponentLinkUnresolved,
        Self::ActRecordUntyped,
        Self::LocalOccurrenceUnplaced,
        Self::ComponentInsertUnplaced,
        Self::ComponentInsertOrderUnknown,
        Self::AssemblyJointUnprojected,
        Self::DrawingDocumentOmitted,
        Self::HistoryBindingBudgetExceeded,
        Self::HistoryBindingWorkExceeded,
        Self::RecipeSideSearchBudgetExceeded,
        Self::RecipePrefixUntyped,
        Self::ParameterOrderBudgetExceeded,
        Self::HistoryRecordFramingFailed,
        Self::DesignBodyBindingUnresolved,
        Self::ReferenceImageNativeRetained,
        Self::DecalNativeRetained,
        Self::EdgeReferenceLostUnrepaired,
        Self::FeatureDefinitionIncomplete,
        Self::FeatureScopeUnprojected,
        Self::FeatureScopePayloadUninterpreted,
        Self::ParameterUnprojected,
        Self::ParameterRecordUnparsed,
        Self::ParameterOwnerUnrecognized,
        Self::ParameterUnitUntyped,
        Self::MaterialDistanceUnitUntyped,
        Self::ImageAssetUntransferred,
        Self::ParameterExpressionUnbound,
        Self::HistoryDependencyUnprojected,
        Self::HistoryDependencyAmbiguous,
        Self::HistoricalFaceSelectionUnbound,
        Self::SketchRelationNativeRetained,
        Self::SketchDimensionNativeRetained,
        Self::SketchPlacementUnprojected,
        Self::SketchPointUnprojected,
        Self::SketchCurveUnprojected,
        Self::SketchSurfaceUnprojected,
        Self::SketchTextUnprojected,
        Self::SketchRelationUnprojected,
        Self::SketchProfileSearchCapped,
        Self::DimensionUnprojected,
        Self::FeatureProfileSelectionNative,
        Self::FeatureProfileArrangementCapped,
        Self::FeatureProfileDensificationCapped,
        Self::FeaturePathSelectionNative,
        Self::FeatureFaceSelectionNative,
        Self::FeatureFaceSelectionActiveSubstituted,
        Self::FeatureBodySelectionNative,
        Self::FeatureFaceOperandUnresolved,
        Self::FeatureEdgeSelectionNative,
        Self::FeatureEdgeOperandUnresolved,
        Self::FeatureEdgeSelectionLost,
        Self::FeaturePatternInstanceSearchCapped,
        Self::BrepBlobUndecoded,
        Self::MeshContainerUnjoined,
        Self::MeshContainerUndecoded,
        Self::MeshContainerMissing,
        Self::MeshAttributeNotTransferred,
        Self::XrefTableUndecoded,
        Self::XrefPlacementUndecoded,
        Self::XrefPlacementSuperseded,
        Self::MeshVertexPrecisionReduced,
        Self::BodylessDesignCarrier,
        Self::AssemblyComponentsExternal,
        Self::NurbsSurfaceCarrier,
        Self::NurbsCurveCarrier,
        Self::SurfaceCacheRejected,
        Self::FaceSurfaceReferenceDangling,
        Self::SurfaceShapeNotDecoded,
        Self::MeshSurfaceSentinel,
        Self::ProceduralCurveUndecoded,
        Self::PcurveUndecoded,
        Self::BlendSupportPartial,
        Self::SolvedRecordUntyped,
        Self::MaterialNotTransferred,
        Self::GeometryNotTransferred,
        Self::TopologyNotTransferred,
        Self::MissingGeometryStream,
        Self::XrefCycle,
        Self::XrefMemberMissing,
        Self::XrefMemberUndecoded,
        Self::TsplineRecordUntyped,
        Self::TsplineCageUndecoded,
        Self::SourcePreservedImageUnavailable,
        Self::SourceDialectUnverified,
        Self::WriterAppearancePropertiesOmitted,
        Self::SourceDialectDisplaced,
        Self::KernelDialectUnverified,
        Self::KernelCarrierUnparseable,
        Self::DialectLayerCollision,
        Self::TextHeaderNoncanonical,
        Self::KernelHeaderToleranceUnresolved,
        Self::KernelHeaderMetadataUnresolved,
        Self::ManifestMetadataUnresolved,
        Self::ArchiveEntryUnreadable,
        Self::WriterNativeRecordsOmitted,
        Self::WriterDesignRecordsOmitted,
        Self::WriterGeometryOmitted,
        Self::WriterAuxiliaryOmitted,
        Self::WriterProceduralReduced,
        Self::WriterNoExportableSolids,
        Self::WriterSubdOmitted,
        Self::WriterAssetOmitted,
        Self::WriterTessellationOmitted,
        Self::WriterPcurveOmitted,
        Self::WriterPmiOmitted,
        Self::WriterProductOmitted,
        Self::WriterSourceAssociationOmitted,
        Self::DesignIntentUndecoded,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::DimensionCompanionUntyped => "dimension.companion-untyped",
            Self::ConfigurationMemberUnassigned => "configuration.member-unassigned",
            Self::ConfigurationActiveCacheUnavailable => "configuration.active-cache-unavailable",
            Self::ConfigurationRuleUnbound => "configuration.rule-unbound",
            Self::ConfigurationParameterOverrideUnbound => {
                "configuration.parameter-override-unbound"
            }
            Self::ConfigurationFeatureSuppressionUnbound => {
                "configuration.feature-suppression-unbound"
            }
            Self::ActComponentLinkUnresolved => "assembly.act-component-link-unresolved",
            Self::ActRecordUntyped => "act.record-untyped",
            Self::LocalOccurrenceUnplaced => "assembly.local-occurrence-unplaced",
            Self::ComponentInsertUnplaced => "assembly.component-insert-unplaced",
            Self::ComponentInsertOrderUnknown => "assembly.component-insert-order-unknown",
            Self::AssemblyJointUnprojected => "assembly.joint-unprojected",
            Self::DrawingDocumentOmitted => "drawing.document-omitted",
            Self::HistoryBindingBudgetExceeded => "history.binding-budget-exceeded",
            Self::HistoryBindingWorkExceeded => "history.binding-work-exceeded",
            Self::RecipeSideSearchBudgetExceeded => "feature.recipe-search-budget-exceeded",
            Self::RecipePrefixUntyped => "dimension.recipe-prefix-untyped",
            Self::ParameterOrderBudgetExceeded => "parameter.order-budget-exceeded",
            Self::HistoryRecordFramingFailed => "history.record-framing-failed",
            Self::DesignBodyBindingUnresolved => "design.body-binding-unresolved",
            Self::ReferenceImageNativeRetained => "appearance.reference-image-native",
            Self::DecalNativeRetained => "appearance.decal-native",
            Self::EdgeReferenceLostUnrepaired => "feature.edge-reference-lost",
            Self::FeatureDefinitionIncomplete => "feature.definition-incomplete",
            Self::FeatureScopePayloadUninterpreted => "feature.scope-payload-uninterpreted",
            Self::FeatureScopeUnprojected => "feature.scope-unprojected",
            Self::ParameterUnprojected => "parameter.unprojected",
            Self::ParameterRecordUnparsed => "parameter.record-unparsed",
            Self::ParameterOwnerUnrecognized => "parameter.owner-unrecognized",
            Self::ParameterUnitUntyped => "parameter.unit-untyped",
            Self::MaterialDistanceUnitUntyped => "material.distance-unit-untyped",
            Self::ImageAssetUntransferred => "appearance.image-asset-untransferred",
            Self::ParameterExpressionUnbound => "parameter.expression-unbound",
            Self::HistoryDependencyUnprojected => "history.dependency-unprojected",
            Self::HistoricalFaceSelectionUnbound => "history.face-selection-unbound",
            Self::HistoryDependencyAmbiguous => "history.dependency-ambiguous",
            Self::SketchRelationNativeRetained => "sketch.relation-native",
            Self::SketchDimensionNativeRetained => "sketch.dimension-native",
            Self::SketchPlacementUnprojected => "sketch.placement-unprojected",
            Self::SketchPointUnprojected => "sketch.point-unprojected",
            Self::SketchCurveUnprojected => "sketch.curve-unprojected",
            Self::SketchSurfaceUnprojected => "sketch.surface-unprojected",
            Self::SketchTextUnprojected => "sketch.text-unprojected",
            Self::SketchRelationUnprojected => "sketch.relation-unprojected",
            Self::SketchProfileSearchCapped => "sketch.profile-search-capped",
            Self::DimensionUnprojected => "dimension.unprojected",
            Self::FeatureProfileSelectionNative => "feature.profile-selection-native",
            Self::FeatureProfileArrangementCapped => "feature.profile-arrangement-capped",
            Self::FeatureProfileDensificationCapped => "feature.profile-densification-capped",
            Self::FeaturePathSelectionNative => "feature.path-selection-native",
            Self::FeatureFaceSelectionNative => "feature.face-selection-native",
            Self::FeatureFaceSelectionActiveSubstituted => {
                "feature.face-selection-active-substituted"
            }
            Self::FeatureBodySelectionNative => "feature.body-selection-native",
            Self::FeatureFaceOperandUnresolved => "feature.face-operand-unresolved",
            Self::FeatureEdgeSelectionNative => "feature.edge-selection-native",
            Self::FeatureEdgeOperandUnresolved => "feature.edge-operand-unresolved",
            Self::FeatureEdgeSelectionLost => "feature.edge-selection-lost",
            Self::FeaturePatternInstanceSearchCapped => "feature.pattern-instance-search-capped",
            Self::BrepBlobUndecoded => "geometry.brep-blob-undecoded",
            Self::MeshContainerUnjoined => "mesh.container-unjoined",
            Self::MeshContainerUndecoded => "mesh.container-undecoded",
            Self::MeshContainerMissing => "mesh.container-missing",
            Self::MeshAttributeNotTransferred => "mesh.attribute-not-transferred",
            Self::XrefTableUndecoded => "xref.table-undecoded",
            Self::XrefPlacementUndecoded => "xref.placement-undecoded",
            Self::XrefPlacementSuperseded => "xref.placement-superseded",
            Self::MeshVertexPrecisionReduced => "mesh.vertex-precision-reduced",
            Self::BodylessDesignCarrier => "design.bodyless-carrier",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::NurbsSurfaceCarrier => "geometry.nurbs-surface-carrier",
            Self::NurbsCurveCarrier => "geometry.nurbs-curve-carrier",
            Self::SurfaceCacheRejected => "geometry.surface-cache-rejected",
            Self::FaceSurfaceReferenceDangling => "geometry.face-surface-dangling",
            Self::SurfaceShapeNotDecoded => "geometry.surface-shape-not-decoded",
            Self::MeshSurfaceSentinel => "geometry.mesh-surface-sentinel",
            Self::ProceduralCurveUndecoded => "geometry.procedural-curve-undecoded",
            Self::PcurveUndecoded => "geometry.pcurve-undecoded",
            Self::BlendSupportPartial => "geometry.blend-support-partial",
            Self::SolvedRecordUntyped => "geometry.solved-record-untyped",
            Self::MaterialNotTransferred => "material.not-transferred",
            Self::GeometryNotTransferred => "geometry.not-transferred",
            Self::TopologyNotTransferred => "topology.not-transferred",
            Self::MissingGeometryStream => "container.missing-geometry-stream",
            Self::XrefCycle => "xref.cycle",
            Self::XrefMemberMissing => "xref.member-missing",
            Self::XrefMemberUndecoded => "xref.member-undecoded",
            Self::TsplineRecordUntyped => "tspline.record-untyped",
            Self::TsplineCageUndecoded => "tspline.cage-undecoded",
            Self::SourcePreservedImageUnavailable => "source.preserved-image-unavailable",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::WriterAppearancePropertiesOmitted => "writer.appearance-properties-omitted",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::KernelCarrierUnparseable => "source.kernel-carrier-unparseable",
            Self::DialectLayerCollision => "source.dialect-layer-collision",
            Self::TextHeaderNoncanonical => "brep.text-header-noncanonical",
            Self::KernelHeaderToleranceUnresolved => "brep.header-tolerance-unresolved",
            Self::KernelHeaderMetadataUnresolved => "brep.header-metadata-unresolved",
            Self::ManifestMetadataUnresolved => "source.manifest-metadata-unresolved",
            Self::ArchiveEntryUnreadable => "source.archive-entry-unreadable",
            Self::WriterNativeRecordsOmitted => "writer.native-records-omitted",
            Self::WriterDesignRecordsOmitted => "writer.design-records-omitted",
            Self::WriterGeometryOmitted => "writer.geometry-omitted",
            Self::WriterAuxiliaryOmitted => "writer.auxiliary-omitted",
            Self::WriterProceduralReduced => "writer.procedural-reduced",
            Self::WriterNoExportableSolids => "writer.no-exportable-solids",
            Self::WriterSubdOmitted => "writer.subd-omitted",
            Self::WriterAssetOmitted => "writer.asset-omitted",
            Self::WriterTessellationOmitted => "writer.tessellation-omitted",
            Self::WriterPcurveOmitted => "writer.pcurve-omitted",
            Self::WriterPmiOmitted => "writer.pmi-omitted",
            Self::WriterProductOmitted => "writer.product-omitted",
            Self::WriterSourceAssociationOmitted => "writer.source-association-omitted",

            Self::DesignIntentUndecoded => "design.intent-undecoded",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::SurfaceCacheRejected => Severity::Error,
            Self::WriterNativeRecordsOmitted => Severity::Warning,
            Self::ImageAssetUntransferred => Severity::Warning,
            Self::WriterDesignRecordsOmitted => Severity::Error,
            Self::WriterGeometryOmitted => Severity::Error,
            Self::WriterAuxiliaryOmitted => Severity::Warning,
            Self::WriterProceduralReduced => Severity::Warning,
            Self::WriterNoExportableSolids => Severity::Warning,
            Self::WriterSubdOmitted => Severity::Warning,
            Self::WriterAssetOmitted => Severity::Warning,
            Self::WriterTessellationOmitted => Severity::Warning,
            Self::WriterPcurveOmitted => Severity::Error,
            Self::WriterPmiOmitted => Severity::Warning,
            Self::WriterProductOmitted => Severity::Warning,
            Self::WriterSourceAssociationOmitted => Severity::Warning,

            Self::WriterAppearancePropertiesOmitted => Severity::Warning,
            Self::BodylessDesignCarrier
            | Self::AssemblyComponentsExternal
            | Self::NurbsSurfaceCarrier
            | Self::NurbsCurveCarrier
            | Self::MeshSurfaceSentinel => Severity::Info,
            Self::HistoryBindingBudgetExceeded
            | Self::HistoryBindingWorkExceeded
            | Self::RecipeSideSearchBudgetExceeded
            | Self::ParameterOrderBudgetExceeded
            | Self::HistoryRecordFramingFailed
            | Self::MeshContainerUndecoded
            | Self::MissingGeometryStream
            | Self::XrefCycle
            | Self::XrefMemberMissing
            | Self::XrefMemberUndecoded
            | Self::TsplineCageUndecoded
            | Self::DesignIntentUndecoded => Severity::Error,
            Self::GeometryNotTransferred
            | Self::TopologyNotTransferred
            | Self::SourcePreservedImageUnavailable => Severity::Blocking,
            Self::DimensionCompanionUntyped
            | Self::RecipePrefixUntyped
            | Self::ConfigurationMemberUnassigned
            | Self::ConfigurationActiveCacheUnavailable
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::ActComponentLinkUnresolved
            | Self::ActRecordUntyped
            | Self::LocalOccurrenceUnplaced
            | Self::ComponentInsertUnplaced
            | Self::ComponentInsertOrderUnknown
            | Self::AssemblyJointUnprojected
            | Self::DrawingDocumentOmitted
            | Self::DesignBodyBindingUnresolved
            | Self::ReferenceImageNativeRetained
            | Self::DecalNativeRetained
            | Self::EdgeReferenceLostUnrepaired
            | Self::FeatureDefinitionIncomplete
            | Self::FeatureScopeUnprojected
            | Self::FeatureScopePayloadUninterpreted
            | Self::ParameterUnprojected
            | Self::ParameterRecordUnparsed
            | Self::ParameterOwnerUnrecognized
            | Self::ParameterUnitUntyped
            | Self::MaterialDistanceUnitUntyped
            | Self::ParameterExpressionUnbound
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::SketchRelationNativeRetained
            | Self::SketchDimensionNativeRetained
            | Self::SketchPlacementUnprojected
            | Self::SketchPointUnprojected
            | Self::SketchCurveUnprojected
            | Self::SketchSurfaceUnprojected
            | Self::SketchTextUnprojected
            | Self::SketchRelationUnprojected
            | Self::SketchProfileSearchCapped
            | Self::DimensionUnprojected
            | Self::FeatureProfileSelectionNative
            | Self::FeatureProfileArrangementCapped
            | Self::FeatureProfileDensificationCapped
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureBodySelectionNative
            | Self::FeatureFaceOperandUnresolved
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost
            | Self::FeaturePatternInstanceSearchCapped
            | Self::BrepBlobUndecoded
            | Self::MeshContainerUnjoined
            | Self::MeshContainerMissing
            | Self::MeshAttributeNotTransferred
            | Self::XrefTableUndecoded
            | Self::XrefPlacementUndecoded
            | Self::XrefPlacementSuperseded
            | Self::MeshVertexPrecisionReduced
            | Self::FaceSurfaceReferenceDangling
            | Self::SurfaceShapeNotDecoded
            | Self::ProceduralCurveUndecoded
            | Self::PcurveUndecoded
            | Self::BlendSupportPartial
            | Self::SolvedRecordUntyped
            | Self::MaterialNotTransferred
            | Self::TsplineRecordUntyped
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::KernelDialectUnverified
            | Self::KernelCarrierUnparseable
            | Self::DialectLayerCollision
            | Self::TextHeaderNoncanonical
            | Self::KernelHeaderToleranceUnresolved
            | Self::KernelHeaderMetadataUnresolved
            | Self::ManifestMetadataUnresolved
            | Self::ArchiveEntryUnreadable => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::WriterNativeRecordsOmitted => LossTaxonomy::PassthroughRecordOmitted,
            Self::WriterDesignRecordsOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterGeometryOmitted => LossTaxonomy::GeometryNotTransferred,
            Self::WriterAuxiliaryOmitted => LossTaxonomy::AttributesNotTransferred,
            Self::WriterProceduralReduced => LossTaxonomy::ProceduralReduced,
            Self::WriterNoExportableSolids => LossTaxonomy::NoExportableSolids,
            Self::WriterSubdOmitted => LossTaxonomy::SubdOmitted,
            Self::WriterAssetOmitted => LossTaxonomy::AssetNotTransferred,
            Self::WriterTessellationOmitted => LossTaxonomy::TessellationOmitted,
            Self::WriterPcurveOmitted => LossTaxonomy::PcurveOmitted,
            Self::WriterPmiOmitted => LossTaxonomy::PmiOmitted,
            Self::WriterProductOmitted => LossTaxonomy::AssemblyComponentsExternal,
            Self::WriterSourceAssociationOmitted => LossTaxonomy::SourceAssociationOmitted,

            Self::WriterAppearancePropertiesOmitted => LossTaxonomy::AppearanceReduced,
            Self::DimensionCompanionUntyped
            | Self::ParameterRecordUnparsed
            | Self::HistoryRecordFramingFailed
            | Self::SolvedRecordUntyped
            | Self::KernelCarrierUnparseable
            | Self::TsplineRecordUntyped => LossTaxonomy::RecordNotTyped,
            Self::ConfigurationMemberUnassigned
            | Self::ConfigurationActiveCacheUnavailable
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::DrawingDocumentOmitted
            | Self::XrefTableUndecoded
            | Self::ActRecordUntyped => LossTaxonomy::MetadataNotTransferred,
            Self::ActComponentLinkUnresolved
            | Self::AssemblyComponentsExternal
            | Self::XrefCycle
            | Self::XrefMemberMissing
            | Self::XrefMemberUndecoded
            | Self::XrefPlacementUndecoded
            | Self::XrefPlacementSuperseded => LossTaxonomy::AssemblyComponentsExternal,
            Self::HistoryBindingBudgetExceeded
            | Self::HistoryBindingWorkExceeded
            | Self::RecipeSideSearchBudgetExceeded
            | Self::ParameterOrderBudgetExceeded
            | Self::RecipePrefixUntyped
            | Self::FeatureDefinitionIncomplete
            | Self::FeatureScopeUnprojected
            | Self::FeatureScopePayloadUninterpreted
            | Self::ParameterUnprojected
            | Self::ParameterOwnerUnrecognized
            | Self::ParameterUnitUntyped
            | Self::ParameterExpressionUnbound
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::SketchRelationNativeRetained
            | Self::SketchDimensionNativeRetained
            | Self::SketchPlacementUnprojected
            | Self::SketchPointUnprojected
            | Self::SketchCurveUnprojected
            | Self::SketchSurfaceUnprojected
            | Self::SketchTextUnprojected
            | Self::SketchRelationUnprojected
            | Self::SketchProfileSearchCapped
            | Self::DimensionUnprojected
            | Self::FeatureProfileSelectionNative
            | Self::FeatureProfileArrangementCapped
            | Self::FeatureProfileDensificationCapped
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureBodySelectionNative
            | Self::FeatureFaceOperandUnresolved
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost
            | Self::FeaturePatternInstanceSearchCapped
            | Self::AssemblyJointUnprojected => LossTaxonomy::FeatureHistoryRetained,
            Self::MaterialDistanceUnitUntyped => LossTaxonomy::MaterialNotTransferred,
            Self::ImageAssetUntransferred => LossTaxonomy::AssetNotTransferred,
            Self::LocalOccurrenceUnplaced
            | Self::ComponentInsertUnplaced
            | Self::ComponentInsertOrderUnknown => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::DesignBodyBindingUnresolved
            | Self::FaceSurfaceReferenceDangling
            | Self::PcurveUndecoded => LossTaxonomy::ReferenceGraphNotClosed,
            Self::ReferenceImageNativeRetained
            | Self::DecalNativeRetained
            | Self::EdgeReferenceLostUnrepaired
            | Self::MeshAttributeNotTransferred => LossTaxonomy::AttributesNotTransferred,
            Self::BrepBlobUndecoded
            | Self::SurfaceShapeNotDecoded
            | Self::GeometryNotTransferred
            | Self::TsplineCageUndecoded => LossTaxonomy::GeometryNotTransferred,
            Self::MeshContainerUnjoined | Self::MeshContainerMissing => {
                LossTaxonomy::AssetNotTransferred
            }
            Self::SurfaceCacheRejected => LossTaxonomy::DecodeDiagnostic,
            Self::MeshContainerUndecoded
            | Self::DialectLayerCollision
            | Self::DesignIntentUndecoded => LossTaxonomy::DecodeDiagnostic,
            Self::MeshVertexPrecisionReduced => LossTaxonomy::MeshVertexPrecision,
            Self::BodylessDesignCarrier
            | Self::NurbsSurfaceCarrier
            | Self::NurbsCurveCarrier
            | Self::MeshSurfaceSentinel => LossTaxonomy::CarrierSummary,
            Self::ProceduralCurveUndecoded | Self::BlendSupportPartial => {
                LossTaxonomy::ProceduralReduced
            }
            Self::MaterialNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::TopologyNotTransferred => LossTaxonomy::TopologyNotTransferred,
            Self::MissingGeometryStream => LossTaxonomy::MissingGeometryStream,
            Self::SourcePreservedImageUnavailable => LossTaxonomy::PreservedSourceUnavailable,
            Self::SourceDialectUnverified | Self::KernelDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::TextHeaderNoncanonical => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::KernelHeaderToleranceUnresolved => LossTaxonomy::GeometryNotTransferred,
            Self::KernelHeaderMetadataUnresolved => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ManifestMetadataUnresolved => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ArchiveEntryUnreadable => LossTaxonomy::RecordNotTyped,
        }
    }

    /// Strict floor pinned by this local code.
    ///
    /// Defaults to the taxonomy floor. `DesignIntentUndecoded` pins Warning
    /// above its tolerable `DecodeDiagnostic` taxonomy: strict mode refuses a
    /// document whose design intent was dropped.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::DesignIntentUndecoded => Some(Severity::Warning),
            other => other.shared_taxonomy().strict_floor(),
        }
    }

    /// Namespaced [`LossKind`] for this local code (taxonomy + pinned floor).
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        NamespacedLossKind::new(
            cadmpeg_ir::loss_namespace!("f3d"),
            self.code(),
            self.shared_taxonomy(),
        )
        .with_strict_floor(self.strict_floor())
        .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `f3d/<local>`. Severity comes from the local
    /// code; the strict floor comes from [`Self::strict_floor`].
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

/// Whether a decode loss leaves source content without a typed representation.
#[cfg(test)]
#[derive(Clone, Copy)]
enum Representation {
    Design,
    Model,
    Both,
    /// The lost neutral projection still has a complete typed native record.
    Typed,
    /// A feature envelope counts only when its operation payload is typed.
    ScopePayload,
    /// Curve identity and framing alone do not represent the curve's shape.
    SketchCurve,
    Metadata,
    Export,
}

/// Every local loss code has an explicit source-content disposition. Codes
/// emitted from native projection-gap counts retain their actual typed rows;
/// raw payloads, unresolved source owners and missing semantic records do not.
#[cfg(test)]
const UNREPRESENTED: &[(F3dLossCode, Representation)] = &[
    (F3dLossCode::ActRecordUntyped, Representation::Both),
    (
        F3dLossCode::DimensionCompanionUntyped,
        Representation::Design,
    ),
    (
        F3dLossCode::ConfigurationActiveCacheUnavailable,
        Representation::Typed,
    ),
    (
        F3dLossCode::ConfigurationMemberUnassigned,
        Representation::Design,
    ),
    (
        F3dLossCode::ConfigurationRuleUnbound,
        Representation::Design,
    ),
    (
        F3dLossCode::ConfigurationParameterOverrideUnbound,
        Representation::Typed,
    ),
    (
        F3dLossCode::ConfigurationFeatureSuppressionUnbound,
        Representation::Typed,
    ),
    (
        F3dLossCode::ActComponentLinkUnresolved,
        Representation::Model,
    ),
    (F3dLossCode::ImageAssetUntransferred, Representation::Model),
    (F3dLossCode::LocalOccurrenceUnplaced, Representation::Typed),
    (F3dLossCode::ComponentInsertUnplaced, Representation::Model),
    (
        F3dLossCode::ComponentInsertOrderUnknown,
        Representation::Model,
    ),
    (F3dLossCode::AssemblyJointUnprojected, Representation::Model),
    (F3dLossCode::DrawingDocumentOmitted, Representation::Model),
    (
        F3dLossCode::HistoryBindingBudgetExceeded,
        Representation::Design,
    ),
    (
        F3dLossCode::HistoryBindingWorkExceeded,
        Representation::Design,
    ),
    (
        F3dLossCode::RecipeSideSearchBudgetExceeded,
        Representation::Design,
    ),
    (
        F3dLossCode::ParameterOrderBudgetExceeded,
        Representation::Typed,
    ),
    (
        F3dLossCode::HistoryRecordFramingFailed,
        Representation::Design,
    ),
    (
        F3dLossCode::DesignBodyBindingUnresolved,
        Representation::Typed,
    ),
    (
        F3dLossCode::ReferenceImageNativeRetained,
        Representation::Model,
    ),
    (F3dLossCode::DecalNativeRetained, Representation::Model),
    (
        F3dLossCode::EdgeReferenceLostUnrepaired,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureDefinitionIncomplete,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureScopeUnprojected,
        Representation::ScopePayload,
    ),
    (
        F3dLossCode::FeatureScopePayloadUninterpreted,
        Representation::Design,
    ),
    (F3dLossCode::RecipePrefixUntyped, Representation::Design),
    (F3dLossCode::ParameterUnprojected, Representation::Typed),
    (F3dLossCode::ParameterRecordUnparsed, Representation::Design),
    (
        F3dLossCode::ParameterOwnerUnrecognized,
        Representation::Design,
    ),
    (F3dLossCode::ParameterUnitUntyped, Representation::Typed),
    (
        F3dLossCode::MaterialDistanceUnitUntyped,
        Representation::Model,
    ),
    (
        F3dLossCode::ParameterExpressionUnbound,
        Representation::Typed,
    ),
    (
        F3dLossCode::HistoryDependencyUnprojected,
        Representation::Typed,
    ),
    (
        F3dLossCode::HistoryDependencyAmbiguous,
        Representation::Typed,
    ),
    (
        F3dLossCode::HistoricalFaceSelectionUnbound,
        Representation::Typed,
    ),
    (
        F3dLossCode::SketchRelationNativeRetained,
        Representation::Typed,
    ),
    (
        F3dLossCode::SketchDimensionNativeRetained,
        Representation::Typed,
    ),
    (
        F3dLossCode::SketchPlacementUnprojected,
        Representation::Typed,
    ),
    (F3dLossCode::SketchPointUnprojected, Representation::Typed),
    (
        F3dLossCode::SketchCurveUnprojected,
        Representation::SketchCurve,
    ),
    (F3dLossCode::SketchSurfaceUnprojected, Representation::Typed),
    (F3dLossCode::SketchTextUnprojected, Representation::Typed),
    (
        F3dLossCode::SketchRelationUnprojected,
        Representation::Typed,
    ),
    (
        F3dLossCode::SketchProfileSearchCapped,
        Representation::Typed,
    ),
    (F3dLossCode::DimensionUnprojected, Representation::Design),
    (
        F3dLossCode::FeatureProfileSelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureProfileArrangementCapped,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureProfileDensificationCapped,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeaturePathSelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureFaceSelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureFaceSelectionActiveSubstituted,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureBodySelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureFaceOperandUnresolved,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureEdgeSelectionNative,
        Representation::Typed,
    ),
    (
        F3dLossCode::FeatureEdgeOperandUnresolved,
        Representation::Typed,
    ),
    (F3dLossCode::FeatureEdgeSelectionLost, Representation::Typed),
    (
        F3dLossCode::FeaturePatternInstanceSearchCapped,
        Representation::Design,
    ),
    (F3dLossCode::BrepBlobUndecoded, Representation::Model),
    (F3dLossCode::MeshContainerUnjoined, Representation::Model),
    (F3dLossCode::MeshContainerUndecoded, Representation::Model),
    (F3dLossCode::MeshContainerMissing, Representation::Model),
    (
        F3dLossCode::MeshAttributeNotTransferred,
        Representation::Model,
    ),
    (F3dLossCode::XrefTableUndecoded, Representation::Model),
    (F3dLossCode::XrefPlacementUndecoded, Representation::Model),
    (F3dLossCode::XrefPlacementSuperseded, Representation::Model),
    (
        F3dLossCode::MeshVertexPrecisionReduced,
        Representation::Typed,
    ),
    (F3dLossCode::BodylessDesignCarrier, Representation::Typed),
    (
        F3dLossCode::AssemblyComponentsExternal,
        Representation::Typed,
    ),
    (F3dLossCode::NurbsSurfaceCarrier, Representation::Typed),
    (F3dLossCode::NurbsCurveCarrier, Representation::Typed),
    (F3dLossCode::SurfaceCacheRejected, Representation::Model),
    (
        F3dLossCode::FaceSurfaceReferenceDangling,
        Representation::Model,
    ),
    (F3dLossCode::SurfaceShapeNotDecoded, Representation::Model),
    (F3dLossCode::MeshSurfaceSentinel, Representation::Typed),
    (F3dLossCode::ProceduralCurveUndecoded, Representation::Model),
    (F3dLossCode::PcurveUndecoded, Representation::Model),
    (F3dLossCode::BlendSupportPartial, Representation::Model),
    (F3dLossCode::SolvedRecordUntyped, Representation::Model),
    (F3dLossCode::MaterialNotTransferred, Representation::Model),
    (F3dLossCode::GeometryNotTransferred, Representation::Model),
    (F3dLossCode::TopologyNotTransferred, Representation::Model),
    (F3dLossCode::MissingGeometryStream, Representation::Model),
    (F3dLossCode::XrefCycle, Representation::Model),
    (F3dLossCode::XrefMemberMissing, Representation::Model),
    (F3dLossCode::XrefMemberUndecoded, Representation::Model),
    (F3dLossCode::TsplineRecordUntyped, Representation::Model),
    (F3dLossCode::TsplineCageUndecoded, Representation::Model),
    (
        F3dLossCode::SourcePreservedImageUnavailable,
        Representation::Export,
    ),
    (
        F3dLossCode::SourceDialectUnverified,
        Representation::Metadata,
    ),
    (
        F3dLossCode::WriterAppearancePropertiesOmitted,
        Representation::Export,
    ),
    (F3dLossCode::SourceDialectDisplaced, Representation::Export),
    (
        F3dLossCode::KernelDialectUnverified,
        Representation::Metadata,
    ),
    (
        F3dLossCode::KernelCarrierUnparseable,
        Representation::Metadata,
    ),
    (F3dLossCode::DialectLayerCollision, Representation::Metadata),
    (
        F3dLossCode::TextHeaderNoncanonical,
        Representation::Metadata,
    ),
    (
        F3dLossCode::KernelHeaderToleranceUnresolved,
        Representation::Model,
    ),
    (
        F3dLossCode::KernelHeaderMetadataUnresolved,
        Representation::Metadata,
    ),
    (
        F3dLossCode::ManifestMetadataUnresolved,
        Representation::Metadata,
    ),
    (F3dLossCode::ArchiveEntryUnreadable, Representation::Both),
    (
        F3dLossCode::WriterNativeRecordsOmitted,
        Representation::Export,
    ),
    (
        F3dLossCode::WriterDesignRecordsOmitted,
        Representation::Export,
    ),
    (F3dLossCode::WriterGeometryOmitted, Representation::Export),
    (F3dLossCode::WriterAuxiliaryOmitted, Representation::Export),
    (F3dLossCode::WriterProceduralReduced, Representation::Export),
    (F3dLossCode::WriterSubdOmitted, Representation::Export),
    (F3dLossCode::WriterAssetOmitted, Representation::Export),
    (
        F3dLossCode::WriterTessellationOmitted,
        Representation::Export,
    ),
    (F3dLossCode::WriterPcurveOmitted, Representation::Export),
    (F3dLossCode::WriterPmiOmitted, Representation::Export),
    (F3dLossCode::WriterProductOmitted, Representation::Export),
    (
        F3dLossCode::WriterSourceAssociationOmitted,
        Representation::Export,
    ),
    (
        F3dLossCode::WriterNoExportableSolids,
        Representation::Export,
    ),
    (F3dLossCode::DesignIntentUndecoded, Representation::Design),
];

/// Classes with source records that have no typed document representation.
#[cfg(test)]
pub(crate) fn unrepresented(
    losses: &[LossNote],
    native: &crate::native::F3dNative,
) -> cadmpeg_ir::document::ArenaClasses {
    use cadmpeg_ir::document::{ArenaClass, ArenaClasses};
    let mut classes = ArenaClasses::default();
    // Framing and references do not type the historical geometry payload.
    if native
        .asm_histories
        .iter()
        .flat_map(crate::history_records::AsmHistory::states)
        .flat_map(|state| &state.records)
        .any(|record| !record.raw_bytes.is_empty())
    {
        classes.insert(ArenaClass::Design);
    }
    // Indexed envelopes and raw recipe words do not supply the operation's
    // semantic payload. Typed recipe clauses, rather than token framing, are
    // required for native representation.
    let raw_recipe = native.design_edge_operands.iter().any(|operand| {
        !operand.recipe_program.is_empty()
            && operand.recipe_structure.is_none()
            && operand.surface_patch_recipe_structure.is_none()
    }) || native.design_face_operands.iter().any(|operand| {
        !operand.recipe_program.is_empty()
            && (operand.recipe_nodes.is_empty()
                || operand
                    .recipe_nodes
                    .iter()
                    .any(|node| node.recipe_structure.is_none()))
    });
    let native_name_only = native.design_parameter_scopes.iter().any(|scope| {
        matches!(
            scope.payload(),
            crate::records::feature::scope::DesignScopePayload::Native(_)
        )
    });
    if raw_recipe || native_name_only {
        classes.insert(ArenaClass::Design);
    }
    for &(code, disposition) in UNREPRESENTED {
        if !losses.iter().any(|loss| loss.code == code.kind()) {
            continue;
        }
        match disposition {
            Representation::Design => {
                classes.insert(ArenaClass::Design);
            }
            Representation::Model => {
                classes.insert(ArenaClass::Model);
            }
            Representation::Both => {
                classes.insert(ArenaClass::Model);
                classes.insert(ArenaClass::Design);
            }
            Representation::ScopePayload => {
                if native
                    .design_parameter_scopes
                    .iter()
                    .any(|scope| scope.payload().is_uninterpreted())
                {
                    classes.insert(ArenaClass::Design);
                }
            }
            Representation::SketchCurve => {
                if native
                    .sketch_curve_identities
                    .iter()
                    .any(|curve| curve.geometry.is_none())
                {
                    classes.insert(ArenaClass::Design);
                }
            }
            Representation::Typed | Representation::Metadata | Representation::Export => {}
        }
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::{unrepresented, F3dLossCode, Representation, UNREPRESENTED};
    use std::collections::BTreeSet;

    #[test]
    fn every_loss_has_a_source_representation_classification() {
        assert_eq!(UNREPRESENTED.len(), F3dLossCode::ALL.len());
        for code in F3dLossCode::ALL {
            assert_eq!(
                UNREPRESENTED
                    .iter()
                    .filter(|(entry, _)| entry == code)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn every_source_only_loss_declares_its_missing_class() {
        use cadmpeg_ir::document::ArenaClass::{Design, Model};
        let native = crate::native::F3dNative::default();
        for &(code, disposition) in UNREPRESENTED {
            let classes = unrepresented(&[code.note("source-only test record")], &native);
            match disposition {
                Representation::Design => assert!(classes.contains(&Design), "{code:?}"),
                Representation::Model => assert!(classes.contains(&Model), "{code:?}"),
                Representation::Both => assert_eq!(classes, [Model, Design].into(), "{code:?}"),
                Representation::Typed | Representation::Metadata | Representation::Export => {
                    assert!(classes.is_empty(), "{code:?}");
                }
                Representation::ScopePayload | Representation::SketchCurve => {}
            }
        }
    }

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = F3dLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "dimension.companion-untyped",
                "configuration.member-unassigned",
                "configuration.active-cache-unavailable",
                "configuration.rule-unbound",
                "configuration.parameter-override-unbound",
                "configuration.feature-suppression-unbound",
                "assembly.act-component-link-unresolved",
                "act.record-untyped",
                "assembly.local-occurrence-unplaced",
                "assembly.component-insert-unplaced",
                "assembly.component-insert-order-unknown",
                "assembly.joint-unprojected",
                "drawing.document-omitted",
                "history.binding-budget-exceeded",
                "history.binding-work-exceeded",
                "feature.recipe-search-budget-exceeded",
                "dimension.recipe-prefix-untyped",
                "parameter.order-budget-exceeded",
                "history.record-framing-failed",
                "design.body-binding-unresolved",
                "appearance.reference-image-native",
                "appearance.decal-native",
                "feature.edge-reference-lost",
                "feature.definition-incomplete",
                "feature.scope-unprojected",
                "feature.scope-payload-uninterpreted",
                "parameter.unprojected",
                "parameter.record-unparsed",
                "parameter.owner-unrecognized",
                "parameter.unit-untyped",
                "material.distance-unit-untyped",
                "appearance.image-asset-untransferred",
                "parameter.expression-unbound",
                "history.dependency-unprojected",
                "history.dependency-ambiguous",
                "history.face-selection-unbound",
                "sketch.relation-native",
                "sketch.dimension-native",
                "sketch.placement-unprojected",
                "sketch.point-unprojected",
                "sketch.curve-unprojected",
                "sketch.surface-unprojected",
                "sketch.text-unprojected",
                "sketch.relation-unprojected",
                "sketch.profile-search-capped",
                "dimension.unprojected",
                "feature.profile-selection-native",
                "feature.profile-arrangement-capped",
                "feature.profile-densification-capped",
                "feature.path-selection-native",
                "feature.face-selection-native",
                "feature.face-selection-active-substituted",
                "feature.body-selection-native",
                "feature.face-operand-unresolved",
                "feature.edge-selection-native",
                "feature.edge-operand-unresolved",
                "feature.edge-selection-lost",
                "feature.pattern-instance-search-capped",
                "geometry.brep-blob-undecoded",
                "mesh.container-unjoined",
                "mesh.container-undecoded",
                "mesh.container-missing",
                "mesh.attribute-not-transferred",
                "xref.table-undecoded",
                "xref.placement-undecoded",
                "xref.placement-superseded",
                "mesh.vertex-precision-reduced",
                "design.bodyless-carrier",
                "assembly.components-external",
                "geometry.nurbs-surface-carrier",
                "geometry.nurbs-curve-carrier",
                "geometry.surface-cache-rejected",
                "geometry.face-surface-dangling",
                "geometry.surface-shape-not-decoded",
                "geometry.mesh-surface-sentinel",
                "geometry.procedural-curve-undecoded",
                "geometry.pcurve-undecoded",
                "geometry.blend-support-partial",
                "geometry.solved-record-untyped",
                "material.not-transferred",
                "geometry.not-transferred",
                "topology.not-transferred",
                "container.missing-geometry-stream",
                "xref.cycle",
                "xref.member-missing",
                "xref.member-undecoded",
                "tspline.record-untyped",
                "tspline.cage-undecoded",
                "source.preserved-image-unavailable",
                "source.dialect-unverified",
                "writer.appearance-properties-omitted",
                "target.source-dialect-displaced",
                "source.kernel-dialect-unverified",
                "source.kernel-carrier-unparseable",
                "source.dialect-layer-collision",
                "brep.text-header-noncanonical",
                "brep.header-tolerance-unresolved",
                "brep.header-metadata-unresolved",
                "source.manifest-metadata-unresolved",
                "source.archive-entry-unreadable",
                "writer.native-records-omitted",
                "writer.design-records-omitted",
                "writer.geometry-omitted",
                "writer.auxiliary-omitted",
                "writer.procedural-reduced",
                "writer.no-exportable-solids",
                "writer.subd-omitted",
                "writer.asset-omitted",
                "writer.tessellation-omitted",
                "writer.pcurve-omitted",
                "writer.pmi-omitted",
                "writer.product-omitted",
                "writer.source-association-omitted",
                "design.intent-undecoded",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in F3dLossCode::ALL {
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
        for code in F3dLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
