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
    /// A decoded operation has operands outside the v11 operation contract.
    FeatureDesignOperationNative,
    /// A sketch relation retains operands whose v11 relation is unresolved.
    SketchDesignConstraintNative,
    /// An optional feature label is empty and has no neutral display name.
    FeatureLabelUnusable,
    /// Sketch curves carry entity flags whose construction meaning is
    /// unresolved.
    SketchConstructionUnresolved,
    /// A distance dimension measured to an axis line stores twice the distance
    /// between its operands; the dimension stays native.
    SketchAxisDistanceNative,
    /// A construction line stores a carrier that its endpoints do not lie on;
    /// the line stays native.
    SketchConstructionCarrierConflict,
    /// A line stores two endpoints at one position; the line stays native.
    SketchDegenerateLineNative,
    /// Distinct line endpoints are read as one position under an assumption of rounding.
    SketchLineEndpointsRoundingAssumed,
    /// A sketch-point paired association has no neutral relation semantics.
    SketchPairedAssociationUnprojected,
    /// A sketch entity retains unresolved curve construction operands.
    SketchCurveNative,
    /// A listed sketch member has unresolved record and curve semantics.
    SketchMemberNative,
    /// A sketch text member record stores no curve; it stays native.
    SketchTextMemberNative,
    /// A sketch box frame or box group record stores no curve; it stays native.
    SketchBoxMemberNative,
    /// A sketch box member record names entities that do not fill the roles
    /// that its layout assigns; the record stays native.
    SketchBoxRoleMismatch,
    /// A planar curve member has no resolved curve rule; it stays native.
    SketchMemberCurveNative,
    /// A polygon control relation names two different vertex references; the
    /// repeated-vertex reading does not apply and both references stay native.
    SketchPolygonControlVerticesDiffer,
    /// A circle parameter record binds a parameter to a circle with an unresolved role.
    SketchCircleParameterNative,
    /// A spline-point relation retains unresolved locus and parameter roles.
    SketchSplinePointNative,
    /// A spline entity retains its source curve pending exact operands.
    SketchSplineCurveNative,
    /// An ellipse retains its directed trim operands natively.
    SketchEllipseCurveNative,
    /// A midpoint relation retains unresolved curve locus semantics.
    SketchMidpointCurveNative,
    /// Profile-body records do not decode, so the profiles that select them
    /// stay native.
    FeatureProfileBodyUndecoded,
    /// A feature label name stores unpaired UTF-16 surrogates, which the
    /// neutral name replaces.
    FeatureLabelNameReplaced,
    /// Typed feature records have an operation graph that is not closed.
    FeatureOperationGraphOpen,
    /// An edge treatment retains an unresolved empty edge-item index list.
    FeatureEdgeSelectionEmptyItemsNative,
    /// An edge treatment retains an unresolved input-edge selection.
    FeatureEdgeSelectionNative,
    /// A fillet omits an edge set whose items all store empty index-reference lists.
    FeatureFilletEmptyEdgeSetsOmitted,
    /// An edge-treatment input edge has several matched owners of identical geometry.
    FeatureEdgeOwnerIdenticalGeometry,
    /// An input edge rests on the assumption that extra selector generators do not change it.
    FeatureEdgeSelectorGeneratorsAssumed,
    /// A Move Face feature retains a face operand that does not join exactly one stored face revision.
    FeatureMoveFaceSelectionNative,
    /// A Thread face stores a cone axis ratio that is not a finite number.
    FeatureFaceJoinThreadRatioNonfinite,
    /// A Thread face differs from the exact surface form within a tolerance.
    FeatureFaceJoinThreadToleranceAssumed,
    /// A selected face rests on the assumption that its matched revision is the input face.
    FeatureFaceJoinInputRevisionAssumed,
    /// A selected face rests on a matching attribute that repeats on its owner.
    FeatureFaceJoinRepeatedAttribute,
    /// A selected face has a generated-name family that shares no feature with its selector.
    FeatureFaceJoinUnsharedFamily,
    /// A selected face has several matched owners of identical surface geometry.
    FeatureFaceJoinIdenticalOwners,
    /// Extrusion construction operands remain native.
    FeatureExtrusionOperandsNative,
    /// An extrusion retains its exact profile selection without a neutral sketch graph.
    FeatureExtrusionProfileNative,
    /// A surface extrusion direction rests on the assumed polarity of Boolean slot 3.
    FeatureSurfaceExtrusionPolarityAssumed,
    /// A through-all surface extrusion reads its neutral extent as bounded by the selected solid body.
    FeatureSurfaceExtrusionSolidBoundAssumed,
    /// A planar extrusion target retains its unresolved body-face identity.
    FeatureExtrusionTargetFaceNative,
    /// Rectangular patterns retain construction operands outside the fixed-spacing feature form.
    FeatureRectangularPatternOperandsNative,
    /// Thread annotations retain class, handedness, termination, and unresolved sizing operands natively.
    FeatureThreadDefinitionPartial,
    /// Thicken selections, construction controls, or thickness side remain native.
    FeatureThickenOperandsNative,
    /// Thicken retains a construction form with unresolved operands.
    FeatureThickenConstructionNative,
    /// Shell operations retain construction controls only natively.
    FeatureShellDefinitionPartial,
    /// Attachment type-9 modes, Boolean controls, and states have no construction map.
    FeatureSheetMetalAttachmentControlsNative,
    /// Separate bends retain unresolved attachment and scalar controls.
    FeatureSheetMetalBendControlsNative,
    /// Corner topology transitions and construction operands remain native.
    FeatureSheetMetalCornerEffectNative,
    /// Sheet-metal corner rounds retain unresolved fillet operands.
    FeatureSheetMetalCornerRoundOperandsNative,
    /// Projected unchanged corners retain construction controls natively.
    FeatureSheetMetalCornerControlsRetained,
    /// Projected attached plates retain construction controls in native records.
    FeatureSheetMetalAttachmentControlsRetained,
    /// Projected bends retain construction controls in native records.
    FeatureSheetMetalBendControlsRetained,
    /// Projected sheet-metal relief and transition forms remain in native style records.
    FeatureSheetMetalStyleFormsNative,
    /// The active unfold rule states no decimal K-factor; sheet-metal rules keep no allowance.
    FeatureSheetMetalUnfoldFactorNative,
    /// Relief forms and unadmitted relief dimensions retain native values.
    FeatureSheetMetalReliefNative,
    /// A style `KFactor` parameter differs from the active unfold-rule K-factor; the rule keeps no allowance.
    FeatureSheetMetalStyleFactorConflict,
    /// No active unfold rule resolves in a part with sheet-metal styles.
    FeatureSheetMetalUnfoldRuleUnresolved,
    /// A sheet-metal rule takes its K-factor from the style `KFactor` parameter without an active unfold rule.
    FeatureSheetMetalStyleFactorAssumed,
    /// An attached plate stays native; the note names the first refused admission check.
    FeatureSheetMetalPlateRefused,
    /// Projected contour flanges retain construction controls and their bend and corner records natively.
    FeatureSheetMetalContourControlsRetained,
    /// The legacy feature record of a contour flange stays native; the note
    /// names the first refused admission check.
    FeatureSheetMetalContourFlangeRefused,
    /// Projected Unfold and Refold features retain controls and face operands natively.
    FeatureSheetMetalFoldControlsRetained,
    /// A Refold record stores no reference to the Unfold whose bends it restores.
    FeatureSheetMetalRefoldSourceUnresolved,
    /// An Unfold or Refold stays native; the note names the first refused admission check.
    FeatureSheetMetalFoldRefused,
    /// A sheet-metal face plate without a bend stays native; the note names the first refused check.
    FeatureSheetMetalFaceRefused,
    /// A sheet-metal plate, bend, or corner stays native without another sheet-metal note; the note names the projection stage.
    FeatureSheetMetalNativeUnexplained,
    /// The edge-naming and stored-edge joins have no records; the note names the reason.
    FeatureEdgeInventoryUnavailable,
    /// A feature-label class has no recognized family; its features stay native and the note names the class.
    FeatureNativeClassUnrecognized,
    /// A sheet-metal style record has a segment version without a field layout; plates that need the style stay native.
    FeatureSheetMetalStyleVersionUnread,
    /// Revolve operands differ from the admitted construction forms.
    FeatureRevolveOperandsNative,
    /// `DeleteFace` Boolean construction roles remain unresolved.
    FeatureDeleteFaceOperandsNative,
    /// Fillet construction operands remain native.
    FeatureFilletOperandsNative,
    /// Chamfer construction operands remain native.
    FeatureChamferOperandsNative,
    /// Mirror operands differ from the identity-frame, zero-option construction.
    FeatureMirrorOperandsNative,
    /// Mirror Boolean construction roles remain unresolved.
    FeatureMirrorBooleanNative,
    /// Mirror instance maps or generated result operands remain native.
    FeatureMirrorInstanceMapNative,
    /// Split construction operands or result topology remain native.
    FeatureSplitOperandsNative,
    /// Split mode 1 retains its side and body-revision operands.
    FeatureSplitModeOneNative,
    /// Split mode 2 operands differ from the selected-face construction.
    FeatureSplitModeTwoNative,
    /// A surface-tool Split retains its unresolved kept side.
    FeatureSplitTrimSideNative,
    /// Selected Split faces retain their historical selector identity.
    FeatureSplitFaceSelectionNative,
    /// A persistent body wrapper has no exact current-model join.
    FeatureBodySelectionNative,
    /// A history work plane's stored in-plane axes are not orthonormal.
    FeatureWorkPlaneFrameNative,
    /// Revolve extents retain unresolved angular termination operands.
    FeatureRevolveExtentNative,
    /// Fixed-angle revolutions retain an unresolved direction control.
    FeatureRevolveDirectionNative,
    /// A projected revolve retains an unresolved Boolean control natively.
    FeatureRevolveControlUnresolved,
    /// Sweep construction controls have no exact neutral interpretation.
    FeatureSweepControlsUnresolved,
    /// Loft construction controls and section conditions have no exact neutral interpretation.
    FeatureLoftControlsUnresolved,
    /// Section scalar operands do not close a positional loft construction.
    FeatureLoftSectionScalarsNative,
    /// Solid-loft operands do not close the positional construction.
    FeatureLoftOperandsNative,
    /// Generated result operands do not close the solid-loft construction.
    FeatureLoftResultOperandsNative,
    /// Surface-loft section selections have no complete neutral construction.
    FeatureSurfaceLoftSectionsNative,
    /// Inventor operations retain unresolved suppression and dependency state.
    FeatureStateUnresolved,
    /// Derived components retain external source construction only natively.
    FeatureDerivedSourceNative,
    /// Derived result-body operands retain unresolved current-model body joins.
    FeatureDerivedResultNative,
    /// Derived-part tagging has no neutral construction rule.
    FeatureDerivedTaggingNative,
    /// Tagging selection effects remain native.
    FeatureTaggingNative,
    /// Stored-body composite selections retain unresolved construction controls.
    FeatureCompositeOperandsNative,
    /// Coil construction controls retain unresolved polarity.
    FeatureCoilOperandsNative,
    /// Parameter records have a unit or expression graph that is not closed.
    ParameterGraphOpen,
    /// Parameter records have an expression whose text passes the local cap.
    ParameterExpressionBounded,
    /// Parameter records whose root literal has the undefined base-unit type carry their value and no expression text.
    ParameterExpressionNative,
    /// Parameter records whose root literal has a base-unit type read by record layout only carry their value and no expression text.
    ParameterUnitLayoutAssumed,
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
    /// Occurrences hidden below a root occurrence stay in native records.
    AssemblyVisibilityNestedNative,
    /// Grounding joints leave the document; at design depth only their
    /// grounded occurrences remain, without the joint ids and native records.
    AssemblyGroundingJointDropped,
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
    /// The kernel carrier's `delta_state` chain does not frame or close.
    KernelHistoryUnclosed,
    /// The kernel carrier's `delta_state` chain passed the local scan cap.
    KernelHistoryBounded,
    /// A feature retains a history state from a naming tag whose stored schema differs from the carrier schema.
    KernelHistoryTagSchemaAssumed,
    /// Split and Base features have no bound history state.
    FeatureHistoryStateUnbound,
    /// A history state could not be materialized or decoded to the body of a
    /// Base or Split feature.
    FeatureHistoryBodyUndecoded,
    /// The body of a Base or Split feature in its history state is not a closed
    /// manifold solid.
    FeatureHistoryBodyNotClosed,
    /// A part passed the local cap of history bodies or of their materialization work.
    FeatureHistoryBodyBounded,
    /// The Base or Split feature of a decoded history body is not a projected
    /// feature, so the body has no origin to name.
    FeatureHistoryBodyUnlinked,
    /// Drawing records retain unresolved semantics natively.
    DrawingNativeOnly,
    /// A drawing record is outside its typed prefix form.
    DrawingRecordMalformed,
    /// A dimension geometry intent names no unique active-body face or edge.
    PmiTargetUnresolved,
    /// A dimension value record stores a tolerance form outside the
    /// established codes.
    PmiToleranceUnresolved,
    /// A dimension owner or value record is outside the established grammar.
    PmiRecordMalformed,
    /// A feature control frame is outside the established grammar.
    PmiFrameNative,
    /// A feature control frame names datums that do not resolve.
    PmiDatumSystemUnresolved,
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
        Self::FeatureDesignOperationNative,
        Self::SketchDesignConstraintNative,
        Self::FeatureLabelUnusable,
        Self::SketchConstructionUnresolved,
        Self::SketchAxisDistanceNative,
        Self::SketchConstructionCarrierConflict,
        Self::SketchDegenerateLineNative,
        Self::SketchLineEndpointsRoundingAssumed,
        Self::SketchPairedAssociationUnprojected,
        Self::SketchCurveNative,
        Self::SketchMemberNative,
        Self::SketchTextMemberNative,
        Self::SketchBoxMemberNative,
        Self::SketchBoxRoleMismatch,
        Self::SketchMemberCurveNative,
        Self::SketchPolygonControlVerticesDiffer,
        Self::SketchCircleParameterNative,
        Self::SketchSplinePointNative,
        Self::SketchSplineCurveNative,
        Self::SketchEllipseCurveNative,
        Self::SketchMidpointCurveNative,
        Self::FeatureProfileBodyUndecoded,
        Self::FeatureLabelNameReplaced,
        Self::FeatureOperationGraphOpen,
        Self::FeatureEdgeSelectionEmptyItemsNative,
        Self::FeatureEdgeSelectionNative,
        Self::FeatureFilletEmptyEdgeSetsOmitted,
        Self::FeatureEdgeOwnerIdenticalGeometry,
        Self::FeatureEdgeSelectorGeneratorsAssumed,
        Self::FeatureMoveFaceSelectionNative,
        Self::FeatureFaceJoinThreadRatioNonfinite,
        Self::FeatureFaceJoinThreadToleranceAssumed,
        Self::FeatureFaceJoinInputRevisionAssumed,
        Self::FeatureFaceJoinRepeatedAttribute,
        Self::FeatureFaceJoinUnsharedFamily,
        Self::FeatureFaceJoinIdenticalOwners,
        Self::FeatureExtrusionOperandsNative,
        Self::FeatureExtrusionProfileNative,
        Self::FeatureSurfaceExtrusionPolarityAssumed,
        Self::FeatureSurfaceExtrusionSolidBoundAssumed,
        Self::FeatureExtrusionTargetFaceNative,
        Self::FeatureRectangularPatternOperandsNative,
        Self::FeatureThreadDefinitionPartial,
        Self::FeatureThickenOperandsNative,
        Self::FeatureThickenConstructionNative,
        Self::FeatureShellDefinitionPartial,
        Self::FeatureSheetMetalAttachmentControlsNative,
        Self::FeatureSheetMetalBendControlsNative,
        Self::FeatureSheetMetalCornerEffectNative,
        Self::FeatureSheetMetalCornerRoundOperandsNative,
        Self::FeatureSheetMetalCornerControlsRetained,
        Self::FeatureSheetMetalAttachmentControlsRetained,
        Self::FeatureSheetMetalBendControlsRetained,
        Self::FeatureSheetMetalStyleFormsNative,
        Self::FeatureSheetMetalUnfoldFactorNative,
        Self::FeatureSheetMetalReliefNative,
        Self::FeatureSheetMetalStyleFactorConflict,
        Self::FeatureSheetMetalUnfoldRuleUnresolved,
        Self::FeatureSheetMetalStyleFactorAssumed,
        Self::FeatureSheetMetalPlateRefused,
        Self::FeatureSheetMetalContourControlsRetained,
        Self::FeatureSheetMetalContourFlangeRefused,
        Self::FeatureSheetMetalFoldControlsRetained,
        Self::FeatureSheetMetalRefoldSourceUnresolved,
        Self::FeatureSheetMetalFoldRefused,
        Self::FeatureSheetMetalFaceRefused,
        Self::FeatureSheetMetalNativeUnexplained,
        Self::FeatureEdgeInventoryUnavailable,
        Self::FeatureNativeClassUnrecognized,
        Self::FeatureSheetMetalStyleVersionUnread,
        Self::FeatureRevolveOperandsNative,
        Self::FeatureDeleteFaceOperandsNative,
        Self::FeatureFilletOperandsNative,
        Self::FeatureChamferOperandsNative,
        Self::FeatureMirrorOperandsNative,
        Self::FeatureMirrorBooleanNative,
        Self::FeatureMirrorInstanceMapNative,
        Self::FeatureSplitOperandsNative,
        Self::FeatureSplitModeOneNative,
        Self::FeatureSplitModeTwoNative,
        Self::FeatureSplitTrimSideNative,
        Self::FeatureSplitFaceSelectionNative,
        Self::FeatureBodySelectionNative,
        Self::FeatureWorkPlaneFrameNative,
        Self::FeatureRevolveExtentNative,
        Self::FeatureRevolveDirectionNative,
        Self::FeatureRevolveControlUnresolved,
        Self::FeatureSweepControlsUnresolved,
        Self::FeatureLoftControlsUnresolved,
        Self::FeatureLoftSectionScalarsNative,
        Self::FeatureLoftOperandsNative,
        Self::FeatureLoftResultOperandsNative,
        Self::FeatureSurfaceLoftSectionsNative,
        Self::FeatureStateUnresolved,
        Self::FeatureDerivedSourceNative,
        Self::FeatureDerivedResultNative,
        Self::FeatureDerivedTaggingNative,
        Self::FeatureTaggingNative,
        Self::FeatureCompositeOperandsNative,
        Self::FeatureCoilOperandsNative,
        Self::ParameterGraphOpen,
        Self::ParameterExpressionBounded,
        Self::ParameterExpressionNative,
        Self::ParameterUnitLayoutAssumed,
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
        Self::AssemblyVisibilityNestedNative,
        Self::AssemblyGroundingJointDropped,
        Self::SourceDialectUnverified,
        Self::KernelDialectUnverified,
        Self::KernelCarrierUnparseable,
        Self::KernelHeaderMetadataUnresolved,
        Self::KernelHeaderToleranceUnresolved,
        Self::ContainerNoncanonical,
        Self::KernelHistoryUnclosed,
        Self::KernelHistoryBounded,
        Self::KernelHistoryTagSchemaAssumed,
        Self::FeatureHistoryStateUnbound,
        Self::FeatureHistoryBodyUndecoded,
        Self::FeatureHistoryBodyNotClosed,
        Self::FeatureHistoryBodyBounded,
        Self::FeatureHistoryBodyUnlinked,
        Self::DrawingNativeOnly,
        Self::DrawingRecordMalformed,
        Self::PmiTargetUnresolved,
        Self::PmiToleranceUnresolved,
        Self::PmiRecordMalformed,
        Self::PmiFrameNative,
        Self::PmiDatumSystemUnresolved,
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
            Self::FeatureDesignOperationNative => "feature.design-operation-native",
            Self::SketchDesignConstraintNative => "sketch.design-constraint-native",
            Self::FeatureRecordMalformed => "feature.record-malformed",
            Self::FeatureLabelUnusable => "feature.label-unusable",
            Self::SketchConstructionUnresolved => "sketch.construction-unresolved",
            Self::SketchAxisDistanceNative => "sketch.axis-distance-native",
            Self::SketchConstructionCarrierConflict => "sketch.construction-carrier-conflict",
            Self::SketchDegenerateLineNative => "sketch.degenerate-line-native",
            Self::SketchLineEndpointsRoundingAssumed => "sketch.line-endpoints-rounding-assumed",
            Self::SketchPairedAssociationUnprojected => "sketch.paired-association-unprojected",
            Self::SketchCurveNative => "sketch.curve-native",
            Self::SketchMemberNative => "sketch.member-native",
            Self::SketchTextMemberNative => "sketch.text-member-native",
            Self::SketchBoxMemberNative => "sketch.box-member-native",
            Self::SketchBoxRoleMismatch => "sketch.box-role-mismatch",
            Self::SketchMemberCurveNative => "sketch.member-curve-native",
            Self::SketchPolygonControlVerticesDiffer => "sketch.polygon-control-vertices-differ",
            Self::SketchCircleParameterNative => "sketch.circle-parameter-native",
            Self::SketchSplinePointNative => "sketch.spline-point-native",
            Self::SketchSplineCurveNative => "sketch.spline-curve-native",
            Self::SketchEllipseCurveNative => "sketch.ellipse-curve-native",
            Self::SketchMidpointCurveNative => "sketch.midpoint-curve-native",
            Self::FeatureProfileBodyUndecoded => "feature.profile-body-undecoded",
            Self::FeatureLabelNameReplaced => "feature.label-name-replaced",
            Self::FeatureRectangularPatternOperandsNative => {
                "feature.rectangular-pattern-operands-native"
            }
            Self::FeatureExtrusionOperandsNative => "feature.extrusion-operands-native",
            Self::FeatureExtrusionProfileNative => "feature.extrusion-profile-native",
            Self::FeatureSurfaceExtrusionPolarityAssumed => {
                "feature.surface-extrusion-polarity-assumed"
            }
            Self::FeatureSurfaceExtrusionSolidBoundAssumed => {
                "feature.surface-extrusion-solid-bound-assumed"
            }
            Self::FeatureExtrusionTargetFaceNative => "feature.extrusion-target-face-native",
            Self::FeatureOperationGraphOpen => "feature.operation-graph-open",
            Self::FeatureEdgeSelectionEmptyItemsNative => {
                "feature.edge-selection-empty-items-native"
            }
            Self::FeatureEdgeSelectionNative => "feature.edge-selection-native",
            Self::FeatureFilletEmptyEdgeSetsOmitted => "feature.fillet-empty-edge-sets-omitted",
            Self::FeatureEdgeOwnerIdenticalGeometry => "feature.edge-owner-identical-geometry",
            Self::FeatureEdgeSelectorGeneratorsAssumed => {
                "feature.edge-selector-generators-assumed"
            }
            Self::FeatureMoveFaceSelectionNative => "feature.move-face-selection-native",
            Self::FeatureFaceJoinThreadRatioNonfinite => "feature.face-join-thread-ratio-nonfinite",
            Self::FeatureFaceJoinThreadToleranceAssumed => {
                "feature.face-join-thread-tolerance-assumed"
            }
            Self::FeatureFaceJoinInputRevisionAssumed => "feature.face-join-input-revision-assumed",
            Self::FeatureFaceJoinRepeatedAttribute => "feature.face-join-repeated-attribute",
            Self::FeatureFaceJoinUnsharedFamily => "feature.face-join-unshared-family",
            Self::FeatureFaceJoinIdenticalOwners => "feature.face-join-identical-owners",
            Self::FeatureThreadDefinitionPartial => "feature.thread-definition-partial",
            Self::FeatureThickenOperandsNative => "feature.thicken-operands-native",
            Self::FeatureThickenConstructionNative => "feature.thicken-construction-native",
            Self::FeatureShellDefinitionPartial => "feature.shell-definition-partial",
            Self::FeatureSheetMetalAttachmentControlsNative => {
                "feature.sheet-metal-attachment-controls-native"
            }
            Self::FeatureSheetMetalBendControlsNative => "feature.sheet-metal-bend-controls-native",
            Self::FeatureSheetMetalCornerEffectNative => "feature.sheet-metal-corner-effect-native",
            Self::FeatureSheetMetalCornerRoundOperandsNative => {
                "feature.sheet-metal-corner-round-operands-native"
            }
            Self::FeatureSheetMetalCornerControlsRetained => {
                "feature.sheet-metal-corner-controls-retained"
            }
            Self::FeatureSheetMetalAttachmentControlsRetained => {
                "feature.sheet-metal-attachment-controls-retained"
            }
            Self::FeatureSheetMetalBendControlsRetained => {
                "feature.sheet-metal-bend-controls-retained"
            }
            Self::FeatureSheetMetalStyleFormsNative => "feature.sheet-metal-style-forms-native",
            Self::FeatureSheetMetalUnfoldFactorNative => "feature.sheet-metal-unfold-factor-native",
            Self::FeatureSheetMetalReliefNative => "feature.sheet-metal-relief-native",
            Self::FeatureSheetMetalStyleFactorConflict => {
                "feature.sheet-metal-style-factor-conflict"
            }
            Self::FeatureSheetMetalUnfoldRuleUnresolved => {
                "feature.sheet-metal-unfold-rule-unresolved"
            }
            Self::FeatureSheetMetalStyleFactorAssumed => "feature.sheet-metal-style-factor-assumed",
            Self::FeatureSheetMetalPlateRefused => "feature.sheet-metal-plate-refused",
            Self::FeatureSheetMetalContourControlsRetained => {
                "feature.sheet-metal-contour-controls-retained"
            }
            Self::FeatureSheetMetalContourFlangeRefused => {
                "feature.sheet-metal-contour-flange-refused"
            }
            Self::FeatureSheetMetalFoldControlsRetained => {
                "feature.sheet-metal-fold-controls-retained"
            }
            Self::FeatureSheetMetalRefoldSourceUnresolved => {
                "feature.sheet-metal-refold-source-unresolved"
            }
            Self::FeatureSheetMetalFoldRefused => "feature.sheet-metal-fold-refused",
            Self::FeatureSheetMetalFaceRefused => "feature.sheet-metal-face-refused",
            Self::FeatureSheetMetalNativeUnexplained => "feature.sheet-metal-native-unexplained",
            Self::FeatureEdgeInventoryUnavailable => "feature.edge-inventory-unavailable",
            Self::FeatureNativeClassUnrecognized => "feature.native-class-unrecognized",
            Self::FeatureSheetMetalStyleVersionUnread => "feature.sheet-metal-style-version-unread",
            Self::FeatureBodySelectionNative => "feature.body-selection-native",
            Self::FeatureMirrorOperandsNative => "feature.mirror-operands-native",
            Self::FeatureMirrorBooleanNative => "feature.mirror-boolean-native",
            Self::FeatureMirrorInstanceMapNative => "feature.mirror-instance-map-native",
            Self::FeatureSplitOperandsNative => "feature.split-operands-native",
            Self::FeatureSplitModeOneNative => "feature.split-mode-one-native",
            Self::FeatureSplitModeTwoNative => "feature.split-mode-two-native",
            Self::FeatureSplitTrimSideNative => "feature.split-trim-side-native",
            Self::FeatureSplitFaceSelectionNative => "feature.split-face-selection-native",
            Self::FeatureWorkPlaneFrameNative => "feature.work-plane-frame-native",
            Self::FeatureFilletOperandsNative => "feature.fillet-operands-native",
            Self::FeatureChamferOperandsNative => "feature.chamfer-operands-native",
            Self::FeatureRevolveOperandsNative => "feature.revolve-operands-native",
            Self::FeatureDeleteFaceOperandsNative => "feature.delete-face-operands-native",
            Self::FeatureRevolveExtentNative => "feature.revolve-extent-native",
            Self::FeatureRevolveDirectionNative => "feature.revolve-direction-native",
            Self::FeatureRevolveControlUnresolved => "feature.revolve-control-unresolved",
            Self::FeatureSweepControlsUnresolved => "feature.sweep-controls-unresolved",
            Self::FeatureLoftControlsUnresolved => "feature.loft-controls-unresolved",
            Self::FeatureLoftSectionScalarsNative => "feature.loft-section-scalars-native",
            Self::FeatureLoftOperandsNative => "feature.loft-operands-native",
            Self::FeatureLoftResultOperandsNative => "feature.loft-result-operands-native",
            Self::FeatureSurfaceLoftSectionsNative => "feature.surface-loft-sections-native",
            Self::FeatureStateUnresolved => "feature.state-unresolved",
            Self::FeatureDerivedSourceNative => "feature.derived-source-native",
            Self::FeatureDerivedResultNative => "feature.derived-result-native",
            Self::FeatureDerivedTaggingNative => "feature.derived-tagging-native",
            Self::FeatureTaggingNative => "feature.tagging-native",
            Self::FeatureCompositeOperandsNative => "feature.composite-operands-native",
            Self::FeatureCoilOperandsNative => "feature.coil-operands-native",
            Self::ParameterGraphOpen => "parameter.graph-open",
            Self::ParameterExpressionBounded => "parameter.expression-bounded",
            Self::ParameterExpressionNative => "parameter.expression-native",
            Self::ParameterUnitLayoutAssumed => "parameter.unit-layout-assumed",
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
            Self::AssemblyVisibilityNestedNative => "assembly.visibility-nested-native",
            Self::AssemblyGroundingJointDropped => "assembly.grounding-joint-dropped",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::KernelCarrierUnparseable => "source.kernel-carrier-unparseable",
            Self::KernelHeaderMetadataUnresolved => "source.kernel-header-metadata-unresolved",
            Self::KernelHeaderToleranceUnresolved => "source.kernel-header-tolerance-unresolved",
            Self::ContainerNoncanonical => "container.noncanonical",
            Self::KernelHistoryUnclosed => "source.kernel-history-unclosed",
            Self::KernelHistoryBounded => "source.kernel-history-bounded",
            Self::KernelHistoryTagSchemaAssumed => "source.kernel-history-tag-schema-assumed",
            Self::FeatureHistoryStateUnbound => "feature.history-state-unbound",
            Self::FeatureHistoryBodyUndecoded => "feature.history-body-undecoded",
            Self::FeatureHistoryBodyNotClosed => "feature.history-body-not-closed",
            Self::FeatureHistoryBodyBounded => "feature.history-body-bounded",
            Self::FeatureHistoryBodyUnlinked => "feature.history-body-unlinked",
            Self::DrawingNativeOnly => "drawing.native-only",
            Self::DrawingRecordMalformed => "drawing.record-malformed",
            Self::PmiTargetUnresolved => "pmi.target-unresolved",
            Self::PmiToleranceUnresolved => "pmi.tolerance-unresolved",
            Self::PmiRecordMalformed => "pmi.record-malformed",
            Self::PmiFrameNative => "pmi.frame-native",
            Self::PmiDatumSystemUnresolved => "pmi.datum-system-unresolved",
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
            Self::PmiTargetUnresolved
            | Self::PmiToleranceUnresolved
            | Self::PmiRecordMalformed
            | Self::PmiFrameNative
            | Self::PmiDatumSystemUnresolved => LossTaxonomy::PmiOmitted,
            Self::GeometryKernelCarrierNotTransferred
            | Self::GeometryProceduralSurfaceNotTransferred => LossTaxonomy::GeometryNotTransferred,
            Self::DrawingNativeOnly | Self::RseSegmentPairUntyped | Self::UfrxSchemaUnsupported => {
                LossTaxonomy::RecordNotTyped
            }
            Self::DrawingRecordMalformed
            | Self::RseMetadataStreamMalformed
            | Self::RseBulkStreamMalformed
            | Self::AssemblyRecordMalformed
            | Self::PresentationRecordMalformed
            | Self::DesignRecordMalformed
            | Self::SketchRecordMalformed
            | Self::FeatureRecordMalformed
            | Self::RseStreamUnpaired
            | Self::PropertySetStreamMalformed
            | Self::ProteinStreamMalformed
            | Self::KernelHistoryUnclosed
            | Self::UfrxTableMalformed => LossTaxonomy::DecodeDiagnostic,
            Self::KernelCarrierUnparseable => LossTaxonomy::RecordNotTyped,
            Self::KernelHeaderMetadataUnresolved | Self::ContainerNoncanonical => {
                LossTaxonomy::NoncanonicalSourceSyntax
            }
            Self::KernelHeaderToleranceUnresolved => LossTaxonomy::GeometryNotTransferred,
            Self::FeatureOperationGraphOpen
            | Self::FeatureEdgeSelectionEmptyItemsNative
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureFilletEmptyEdgeSetsOmitted
            | Self::FeatureEdgeOwnerIdenticalGeometry
            | Self::FeatureEdgeSelectorGeneratorsAssumed
            | Self::FeatureMoveFaceSelectionNative
            | Self::FeatureFaceJoinThreadRatioNonfinite
            | Self::FeatureFaceJoinThreadToleranceAssumed
            | Self::FeatureFaceJoinInputRevisionAssumed
            | Self::FeatureFaceJoinRepeatedAttribute
            | Self::FeatureFaceJoinUnsharedFamily
            | Self::FeatureFaceJoinIdenticalOwners
            | Self::FeatureExtrusionOperandsNative
            | Self::FeatureExtrusionProfileNative
            | Self::FeatureSurfaceExtrusionPolarityAssumed
            | Self::FeatureSurfaceExtrusionSolidBoundAssumed
            | Self::FeatureExtrusionTargetFaceNative
            | Self::FeatureRectangularPatternOperandsNative
            | Self::FeatureThreadDefinitionPartial
            | Self::FeatureThickenOperandsNative
            | Self::FeatureThickenConstructionNative
            | Self::FeatureDesignOperationNative
            | Self::SketchDesignConstraintNative
            | Self::FeatureShellDefinitionPartial
            | Self::FeatureSheetMetalAttachmentControlsNative
            | Self::FeatureSheetMetalBendControlsNative
            | Self::FeatureSheetMetalCornerEffectNative
            | Self::FeatureSheetMetalCornerRoundOperandsNative
            | Self::FeatureSheetMetalCornerControlsRetained
            | Self::FeatureSheetMetalAttachmentControlsRetained
            | Self::FeatureSheetMetalBendControlsRetained
            | Self::FeatureSheetMetalStyleFormsNative
            | Self::FeatureSheetMetalUnfoldFactorNative
            | Self::FeatureSheetMetalReliefNative
            | Self::FeatureSheetMetalStyleFactorConflict
            | Self::FeatureSheetMetalUnfoldRuleUnresolved
            | Self::FeatureSheetMetalStyleFactorAssumed
            | Self::FeatureSheetMetalPlateRefused
            | Self::FeatureSheetMetalContourControlsRetained
            | Self::FeatureSheetMetalContourFlangeRefused
            | Self::FeatureSheetMetalFoldControlsRetained
            | Self::FeatureSheetMetalRefoldSourceUnresolved
            | Self::FeatureSheetMetalFoldRefused
            | Self::FeatureSheetMetalFaceRefused
            | Self::FeatureSheetMetalNativeUnexplained
            | Self::FeatureEdgeInventoryUnavailable
            | Self::FeatureNativeClassUnrecognized
            | Self::FeatureSheetMetalStyleVersionUnread
            | Self::FeatureBodySelectionNative
            | Self::FeatureMirrorOperandsNative
            | Self::FeatureMirrorBooleanNative
            | Self::FeatureMirrorInstanceMapNative
            | Self::FeatureSplitOperandsNative
            | Self::FeatureSplitModeOneNative
            | Self::FeatureSplitModeTwoNative
            | Self::FeatureSplitTrimSideNative
            | Self::FeatureSplitFaceSelectionNative
            | Self::FeatureWorkPlaneFrameNative
            | Self::FeatureFilletOperandsNative
            | Self::FeatureChamferOperandsNative
            | Self::FeatureRevolveOperandsNative
            | Self::FeatureDeleteFaceOperandsNative
            | Self::FeatureRevolveExtentNative
            | Self::FeatureRevolveDirectionNative
            | Self::FeatureRevolveControlUnresolved
            | Self::FeatureSweepControlsUnresolved
            | Self::FeatureLoftControlsUnresolved
            | Self::FeatureLoftSectionScalarsNative
            | Self::FeatureLoftOperandsNative
            | Self::FeatureLoftResultOperandsNative
            | Self::FeatureSurfaceLoftSectionsNative
            | Self::FeatureProfileBodyUndecoded
            | Self::SketchConstructionUnresolved
            | Self::SketchAxisDistanceNative
            | Self::SketchConstructionCarrierConflict
            | Self::SketchDegenerateLineNative
            | Self::SketchLineEndpointsRoundingAssumed
            | Self::SketchPairedAssociationUnprojected
            | Self::SketchCurveNative
            | Self::SketchMemberNative
            | Self::SketchTextMemberNative
            | Self::SketchBoxMemberNative
            | Self::SketchBoxRoleMismatch
            | Self::SketchMemberCurveNative
            | Self::SketchPolygonControlVerticesDiffer
            | Self::SketchCircleParameterNative
            | Self::SketchSplinePointNative
            | Self::SketchSplineCurveNative
            | Self::SketchEllipseCurveNative
            | Self::SketchMidpointCurveNative
            | Self::FeatureStateUnresolved
            | Self::FeatureDerivedSourceNative
            | Self::FeatureDerivedResultNative
            | Self::FeatureDerivedTaggingNative
            | Self::FeatureCompositeOperandsNative
            | Self::FeatureCoilOperandsNative
            | Self::FeatureTaggingNative
            | Self::KernelHistoryBounded
            | Self::KernelHistoryTagSchemaAssumed
            | Self::FeatureHistoryStateUnbound
            | Self::FeatureHistoryBodyUndecoded
            | Self::FeatureHistoryBodyNotClosed
            | Self::FeatureHistoryBodyBounded
            | Self::FeatureHistoryBodyUnlinked
            | Self::SketchGraphOpen => LossTaxonomy::FeatureHistoryRetained,
            Self::ParameterGraphOpen
            | Self::ParameterExpressionBounded
            | Self::ParameterExpressionNative
            | Self::ParameterUnitLayoutAssumed
            | Self::ParameterScopeUnresolved => LossTaxonomy::ParametricRecordOmitted,
            Self::MetadataPropertyUnmapped
            | Self::FeatureLabelUnusable
            | Self::FeatureLabelNameReplaced
            | Self::AssemblyGroundingJointDropped => LossTaxonomy::MetadataNotTransferred,
            Self::ProteinCatalogUndecodable
            | Self::ProteinAssetRejected
            | Self::ProteinAppearanceAbsent
            | Self::AppearanceDefaultUnresolved
            | Self::ProteinGuidAmbiguous
            | Self::MaterialDistanceUnitUntyped
            | Self::AppearanceFaceOverrideUnresolved => LossTaxonomy::MaterialNotTransferred,
            Self::UfrxSchemaUnsupportedAssembly
            | Self::AssemblyComponentExternal
            | Self::AssemblyVisibilityNestedNative => LossTaxonomy::AssemblyComponentsExternal,
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
    /// code.
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
                "feature.design-operation-native",
                "sketch.design-constraint-native",
                "feature.label-unusable",
                "sketch.construction-unresolved",
                "sketch.axis-distance-native",
                "sketch.construction-carrier-conflict",
                "sketch.degenerate-line-native",
                "sketch.line-endpoints-rounding-assumed",
                "sketch.paired-association-unprojected",
                "sketch.curve-native",
                "sketch.member-native",
                "sketch.text-member-native",
                "sketch.box-member-native",
                "sketch.box-role-mismatch",
                "sketch.member-curve-native",
                "sketch.polygon-control-vertices-differ",
                "sketch.circle-parameter-native",
                "sketch.spline-point-native",
                "sketch.spline-curve-native",
                "sketch.ellipse-curve-native",
                "sketch.midpoint-curve-native",
                "feature.profile-body-undecoded",
                "feature.label-name-replaced",
                "feature.operation-graph-open",
                "feature.edge-selection-empty-items-native",
                "feature.edge-selection-native",
                "feature.fillet-empty-edge-sets-omitted",
                "feature.edge-owner-identical-geometry",
                "feature.edge-selector-generators-assumed",
                "feature.move-face-selection-native",
                "feature.face-join-thread-ratio-nonfinite",
                "feature.face-join-thread-tolerance-assumed",
                "feature.face-join-input-revision-assumed",
                "feature.face-join-repeated-attribute",
                "feature.face-join-unshared-family",
                "feature.face-join-identical-owners",
                "feature.extrusion-operands-native",
                "feature.extrusion-profile-native",
                "feature.surface-extrusion-polarity-assumed",
                "feature.surface-extrusion-solid-bound-assumed",
                "feature.extrusion-target-face-native",
                "feature.rectangular-pattern-operands-native",
                "feature.thread-definition-partial",
                "feature.thicken-operands-native",
                "feature.thicken-construction-native",
                "feature.shell-definition-partial",
                "feature.sheet-metal-attachment-controls-native",
                "feature.sheet-metal-bend-controls-native",
                "feature.sheet-metal-corner-effect-native",
                "feature.sheet-metal-corner-round-operands-native",
                "feature.sheet-metal-corner-controls-retained",
                "feature.sheet-metal-attachment-controls-retained",
                "feature.sheet-metal-bend-controls-retained",
                "feature.sheet-metal-style-forms-native",
                "feature.sheet-metal-unfold-factor-native",
                "feature.sheet-metal-relief-native",
                "feature.sheet-metal-style-factor-conflict",
                "feature.sheet-metal-unfold-rule-unresolved",
                "feature.sheet-metal-style-factor-assumed",
                "feature.sheet-metal-plate-refused",
                "feature.sheet-metal-contour-controls-retained",
                "feature.sheet-metal-contour-flange-refused",
                "feature.sheet-metal-fold-controls-retained",
                "feature.sheet-metal-refold-source-unresolved",
                "feature.sheet-metal-fold-refused",
                "feature.sheet-metal-face-refused",
                "feature.sheet-metal-native-unexplained",
                "feature.edge-inventory-unavailable",
                "feature.native-class-unrecognized",
                "feature.sheet-metal-style-version-unread",
                "feature.revolve-operands-native",
                "feature.delete-face-operands-native",
                "feature.fillet-operands-native",
                "feature.chamfer-operands-native",
                "feature.mirror-operands-native",
                "feature.mirror-boolean-native",
                "feature.mirror-instance-map-native",
                "feature.split-operands-native",
                "feature.split-mode-one-native",
                "feature.split-mode-two-native",
                "feature.split-trim-side-native",
                "feature.split-face-selection-native",
                "feature.body-selection-native",
                "feature.work-plane-frame-native",
                "feature.revolve-extent-native",
                "feature.revolve-direction-native",
                "feature.revolve-control-unresolved",
                "feature.sweep-controls-unresolved",
                "feature.loft-controls-unresolved",
                "feature.loft-section-scalars-native",
                "feature.loft-operands-native",
                "feature.loft-result-operands-native",
                "feature.surface-loft-sections-native",
                "feature.state-unresolved",
                "feature.derived-source-native",
                "feature.derived-result-native",
                "feature.derived-tagging-native",
                "feature.tagging-native",
                "feature.composite-operands-native",
                "feature.coil-operands-native",
                "parameter.graph-open",
                "parameter.expression-bounded",
                "parameter.expression-native",
                "parameter.unit-layout-assumed",
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
                "assembly.visibility-nested-native",
                "assembly.grounding-joint-dropped",
                "source.dialect-unverified",
                "source.kernel-dialect-unverified",
                "source.kernel-carrier-unparseable",
                "source.kernel-header-metadata-unresolved",
                "source.kernel-header-tolerance-unresolved",
                "container.noncanonical",
                "source.kernel-history-unclosed",
                "source.kernel-history-bounded",
                "source.kernel-history-tag-schema-assumed",
                "feature.history-state-unbound",
                "feature.history-body-undecoded",
                "feature.history-body-not-closed",
                "feature.history-body-bounded",
                "feature.history-body-unlinked",
                "drawing.native-only",
                "drawing.record-malformed",
                "pmi.target-unresolved",
                "pmi.tolerance-unresolved",
                "pmi.record-malformed",
                "pmi.frame-native",
                "pmi.datum-system-unresolved",
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
