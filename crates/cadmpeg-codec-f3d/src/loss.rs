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
    /// A counted display override candidate fails its local grammar.
    SceneOverrideRefused,
    UnfoldAllBendsFlagAssumed,
    UnfoldControlUnresolved,
    UnfoldSelectionSlotsAssumed,
    UnfoldSelectionNative,
    UnfoldEnvelopeUnresolved,
    UnfoldMaterialScalarFactorAssumed,
    UnfoldRuleFactorUnresolved,
    UnfoldSettingsRuleReferenceAssumed,
    RefoldGroupSourceAssumed,
    RefoldSourceUnresolved,
    BendLineOwnerOrderAssumed,
    BendStationaryRoleAssumed,
    BendAngleUnresolved,
    BendRadiusFirstAssumed,
    BendStationarySelectionNative,
    BendRuleRadiusUnresolved,
    BendAngleOwnerBacklinkAssumed,
    BendCachedOwnerAngleAssumed,
    /// A displayed path matches several neutral occurrences.
    SceneOverrideAmbiguous,
    /// A displayed path has no neutral occurrence.
    SceneOverrideUnbound,
    /// A direct operation-table pose is applied uniformly to its definition.
    OperationPoseApplied,
    /// A scope namespace has no unique neutral product definition.
    ScopeOwnerUnresolved,
    /// A history output body belongs to another product definition.
    ScopeOutputOwnerMismatch,
    /// Unadmitted output membership prevents a unique history product owner.
    ScopeOutputOwnerUnresolved,
    /// A bound sketch collection belongs to another product definition.
    ScopeSketchOwnerMismatch,
    /// Component-path constraints prevent a uniform operation-table pose.
    OperationPoseDisagrees,
    /// An operation table has no unique admitted neutral placement.
    OperationPoseNative,
    /// Payload-bearing Design dimension companions have no typed locus frame.
    DimensionCompanionUntyped,
    /// A whole-body reference tail has no admitted standard reference-run frame.
    BodyReferenceTailUntyped,
    BodyRecipeSerializationFormAssumed,
    /// A placement construction has no unambiguous registered member layout.
    PlacementConstructionUntyped,
    /// A work plane has no unique checked solved placement.
    WorkPlaneFrameUnbound,
    /// A work plane takes its stored matrix despite nonzero inferred-axis bytes.
    WorkPlaneInferredAxisAssumed,
    /// A cylinder primitive has no unique checked solved placement.
    CylinderPlacementUnbound,
    SpherePrefixFramingAssumed,
    SpherePlacementFlagsAssumed,
    SphereLocalMatrixFramingAssumed,
    SphereLocalFrameAssumed,
    SpherePlacementFrameAssumed,
    SphereDiameterOwnerAssumed,
    SphereDiameterSourceAssumed,

    /// A construction tracking path has no matching indexed type registration.
    ConstructionTrackingPathUntyped,
    ConstructionClosingFlagAssumed,
    /// A Chamfer's non-face group is read as its edge group despite its role.
    ChamferEdgeRoleAssumed,
    ChamferDefinitionUnresolved,
    ChamferSelectionUnresolved,
    ChamferReferenceFaceAssumed,
    ChamferAngleReferenceAssumed,
    ChamferCornerTreatmentUnresolved,
    ChamferTangentChainAssumed,
    ChamferParameterOwnerAssumed,
    ChamferParameterValueAssumed,
    ChamferParameterKindAssumed,
    ChamferParameterUnitAssumed,
    /// A Coil's Boolean target group whose member count is not one is read as one body selection.
    CoilTargetMembersAssumed,
    /// Design configuration JSON members have no assigned neutral semantics.
    ConfigurationMemberUnassigned,
    /// Design configuration rules have no unambiguous activation target.
    ConfigurationRuleUnbound,
    /// Design configuration parameter overrides have no parameter identity.
    ConfigurationParameterOverrideUnbound,
    /// Design configuration feature states have no feature identity.
    ConfigurationFeatureSuppressionUnbound,
    /// An active configuration's suppressed state is inferred while a
    /// suppression column of its row names no feature.
    ConfigurationSuppressionInferred,
    /// Design configuration root records name no active row of a unique table document.
    ConfigurationActiveRowUnbound,
    /// ACT component links have no resolved neutral product role.
    ActComponentLinkUnresolved,
    /// Local component occurrences whose container or definition the source does not settle.
    LocalOccurrenceUnplaced,
    /// A local occurrence owner joins no unique component definition.
    LocalOccurrenceOwnerUnbound,
    /// A solved body has no unique explicit component owner.
    ComponentBodyOwnerUnresolved,
    /// A body without explicit membership uses entity-cluster component ownership.
    ComponentBodyOwnerFallback,
    /// The component owner graph contains a cycle.
    LocalOccurrenceOwnerCycle,
    /// Repeated local instance trees exceed the decode limits.
    LocalOccurrenceExpansionLimit,
    /// A component feature names no unique expanded occurrence.
    FeatureOccurrencePathUnresolved,
    /// A sketch collection owner does not bind one component definition.
    SketchOwnerUnresolved,
    /// Sketches take the Feature label composed from the scope kind spelling
    /// and feature ordinal instead of a stored name.
    SketchNameComposedFromFeatureOrdinal,
    SketchMembershipConflict,
    SketchFolderClosingReference,
    SketchFolderUnprojected,
    /// Sketch geometry backlinks name a sketch geometry group whose member run omits the record.
    SketchGeometryGroupMemberUnlisted,
    SketchGeometryGroupMemberUnbacklinked,
    /// Unresolved `Component Insert` occurrences whose placement has no finite millimetre transform.
    ComponentInsertUnplaced,
    /// Unresolved `Component Insert` occurrences dropped because no authored timeline order tells
    /// top-level inserts from internal operands.
    ComponentInsertOrderUnknown,
    ComponentInsertContainerAssumed,
    /// Component definitions stored as revised records that each bind a context
    /// UUID; the earliest record defines the component.
    ComponentDefinitionRevised,
    /// Registered component entities with no context-UUID binding in this
    /// document; their naming spaces are dropped.
    ComponentNamingSpaceUnbound,
    /// An empty-path root frame has no decoded component-definition reference.
    RootInstanceDefinitionUnbound,
    /// Alternate occurrence frames without a unique primary-indexed selection.
    LocalOccurrenceEnvelopeSuperseded,
    /// A registered unnamed local occurrence has an unresolved envelope.
    LocalOccurrenceEnvelopeNative,
    /// A component-path record with only local elements matches no admitted
    /// local path form.
    LocalComponentPathNative,
    /// A leading-flag occurrence frame whose record the occurrence-placement
    /// type row does not register.
    LocalOccurrenceTypeUnregistered,
    /// An unregistered occurrence frame yields to a registered frame of the
    /// same occurrence.
    LocalOccurrenceYieldedToRegistered,
    /// Two or more occurrence frames of one Design stream state one occurrence
    /// key.
    LocalOccurrenceKeyRepeated,
    /// A class-256 occurrence frame stores a matrix at ordinal one.
    LocalOccurrenceFirstMatrixRefused,
    /// An identity carrier has an unresolved later ordinal.
    LocalOccurrenceIdentityOrdinalUnresolved,
    /// An attributed occurrence whose source reference names no record of the occurrence source
    /// type in its Design segment; the occurrence projects from its frame.
    LocalOccurrenceSourceUnregistered,
    /// Saved free-edge bodies without a product-definition owner.
    SavedFreeEdgeBodyUnowned,
    /// Design assembly joint scopes whose operands, frames, limits, or alignment values do not
    /// project.
    AssemblyJointUnprojected,
    /// Partial assembly alignments with unresolved motion binding or operand paths.
    AssemblyAlignmentPartial,
    /// An ordinary rigid-group frame has no admitted grammar.
    AssemblyRigidGroupFrameUnparsed,
    /// Design assembly joints projected as fixed because their freedom codes or
    /// limit lanes do not map to a joint kind.
    AssemblyJointMotionUndecoded,
    /// Projected Design assembly joints whose stored rest is malformed or
    /// enabled. The neutral joint carries no rest; the native record retains
    /// the valid rest or the malformed-rest mark.
    AssemblyJointRestUnprojected,
    /// Projected Design assembly joints whose activation state is assumed
    /// active because the scope-tail booleans' activation meaning is unknown.
    AssemblyJointActivationAssumed,
    /// Rotation limits retained natively because the zero-lane freedom value is unsettled.
    AssemblyJointAngularLimitsNative,
    /// Projected Design assembly joints whose operand paths carry occurrence
    /// identity pairs of unknown effect on the joint, retained natively.
    AssemblyOperandPathPairsNative,
    /// Root operand identity GUIDs retained natively without a neutral interpretation.
    AssemblyRootPathIdentitiesNative,
    /// A projected joint operand descent selects multiple merged child occurrences.
    AssemblyOperandDescentAmbiguous,
    /// A complete joint occurrence path has no unique merged insertion instance.
    AssemblyOperandDescentUnresolved,
    AssemblyJointSourceBindingAssumed,
    AssemblyConnectorOperandsAssumed,
    AssemblyMotionBindingAssumed,
    AssemblyAdditionalOwnerMotionAssumed,
    AssemblyExtendedTailAssumed,
    AssemblyExtendedTypedTailAssumed,
    /// A joint operand path continues inside an unmerged inserted document; the
    /// remaining keys stay a nested occurrence path.
    AssemblyOperandNestedPathRetained,
    /// An F3Z drawing root was omitted while its derived 3D model transferred.
    DrawingDocumentOmitted,
    /// ASM history estimated retained size exceeded the session's remaining retained-byte allowance.
    HistoryBindingBudgetExceeded,
    /// ASM history topology binding exceeded its local work cap.
    HistoryBindingWorkExceeded,
    /// A Design recipe side-sequence search exceeded its local work cap.
    RecipeSideSearchBudgetExceeded,
    /// Ordering one owner scope's Design parameters exceeded its local work cap.
    ParameterOrderBudgetExceeded,
    /// An ASM history span stays opaque because record framing failed.
    HistoryRecordFramingFailed,
    /// Design body-map pairs do not resolve to a body in the named BREP blob.
    DesignBodyBindingUnresolved,
    /// Design body-map pairs name a body outside the current model.
    HistoryOnlyBodyOmitted,
    /// Reference-image timeline objects retain native Canvas records.
    ReferenceImageNativeRetained,
    ReferenceImageFlagAssumed,
    /// Decal timeline objects retain native image and mapping records.
    DecalNativeRetained,
    DecalMappingAssumed,
    DecalTargetUnresolved,
    DecalImageUnresolved,
    DecalDefinitionUnresolved,
    /// Source parametric edge references marked lost have no independent proof.
    EdgeReferenceLostUnrepaired,
    /// Feature scopes have no complete neutral feature definition.
    FeatureDefinitionIncomplete,
    FeatureMeshImportTessellationsUnresolved,
    FeatureMeshImportEmptySourceAssumed,
    FeatureMeshImportSourceUnparsed,
    FeatureMeshSectionPlaneUnresolved,
    FeatureMeshShellControlsUnresolved,
    FeatureMeshOperationTailPartial,
    FeatureMeshBodySelectionAssumed,
    FeatureMeshOperationSourceUnresolved,
    FeatureMeshOperationScopeUnparsed,
    FeatureMeshOperationSelectorUnruled,
    FeatureMeshOperationParameterUnbound,
    FeatureMeshOperationFieldsUninterpreted,
    FeatureMeshSectionCurvesUnresolved,
    FeatureMeshConversionScopeUnparsed,
    FeatureMeshConversionTailPartial,
    FeatureMeshConversionMethodUnresolved,
    FeatureMeshConversionMethodAssumed,
    FeatureMeshConversionEnvelopeAssumed,
    FeatureMeshConversionSourceUnresolved,
    FeatureMeshConversionOutputUnresolved,
    /// A historical Thread selection has no complete proof of its live faces.
    ThreadLiveFaceUnproven,
    /// A stored-body operation has an empty proven body set.
    FeatureStoredBodiesEmpty,
    /// A decoded body snapshot supplies no construction recipe.
    FeatureStoredBodiesRecipeAbsent,
    /// A Base Feature has no complete unique body binding.
    FeatureStoredBodiesUnbound,
    FeatureDerivedInstanceComponentInsertAssumed,
    FeatureComponentInsertMembershipAssumed,
    FeatureDerivedInstanceSourcesUnresolved,
    FeatureDerivedInstanceCarrierPlacementAssumed,
    FeatureDerivedInstanceScopePlacementAssumed,
    FeatureDerivedInstanceContextTransformAssumed,
    FeatureDerivedInstancePlacementUnrepresentable,
    FeatureDerivedInstanceDefinitionUnresolved,
    /// A paste keeps its bodies; its source pairs or a source node do not resolve.
    FeaturePasteSourcesUnresolved,
    FeatureOccurrenceTransferSourceUnresolved,
    FeatureOccurrenceTransferOwnerUnresolved,
    FeatureOccurrenceTransferOccurrenceUnresolved,
    FeatureOccurrenceTransferPlacementUnresolved,
    /// Several timeline objects name one owned scope; the lowest readable record owns it.
    FeatureTimelineOwnerCompeting,
    FeatureTimelineNullSlotAssumed,
    /// A timeline owner has no same-index paired header; its copy form stays unread.
    FeatureTimelineOwnerUnpaired,
    /// A pasted copy is assumed to keep its source placement.
    FeaturePasteCopyPlacementAssumed,
    /// A Mirror plane is read from the `WorkPlane` scope after its plane
    /// selection identity.
    FeatureMirrorPlaneWorkPlaneAssumed,
    /// A circular pattern's symmetric distribution has no circular pattern field.
    FeatureCircularPatternPlacementNotTransferred,
    /// A partial-turn circular pattern without a readable symmetric byte is read as one-sided.
    FeatureCircularPatternDistributionAssumed,
    /// A feature scope has no complete unique ASM output transition interval.
    FeatureOutputStateChainUnbound,
    /// An affected body slot has no unique active body in the selected BREP.
    FeatureOutputBodyUnbound,
    /// A derived base scope stores a copy of a source and frame that the archive does not contain.
    FeatureDerivedBaseUnresolved,
    /// A Pipe section is inferred as a circle from its result faces.
    FeaturePipeCircularSectionInferred,
    /// A Pipe whose fractions both equal one is assumed to sweep its whole path group.
    FeaturePipeWholePathExtentAssumed,
    /// A Pipe section without an unchanged new-body result is assumed to be a circle.
    FeaturePipeCircularSectionAssumed,
    /// A Pipe with one unit and one zero fraction is assumed to start its section at a path end.
    FeaturePipePathEndSectionAssumed,
    FeatureSweepSourceUnavailable,
    FeatureSweepOperationPrefixAssumed,
    FeatureSweepOperationUnresolved,
    FeatureSweepSectionRoleAssumed,
    FeatureSweepProfileCarrierAssumed,
    FeatureSweepSelectionUnresolved,
    FeatureSweepScalarUnresolved,
    FeatureSweepTargetsNative,
    FeatureSweepResultModeUnresolved,
    FeatureSweepResultModeInferred,
    FeatureSweepPathTangentAssumedFalse,
    FeatureSweepLinearizeAssumedFalse,
    FeatureSweepControlsUninterpreted,
    FeaturePipeSourceUnresolved,
    FeaturePipeOperationUnresolved,
    FeaturePipeHollowUnresolved,
    FeaturePipeScalarUnresolved,
    FeaturePipeSectionUnresolved,
    FeaturePipePathUnresolved,
    FeaturePipePathExtentUnresolved,
    FeaturePipePathParameterizationAssumed,
    FeaturePipePathRoleAssumed,
    FeaturePipeTargetsRetainedNative,
    FeaturePipeTargetsUnresolved,
    FeaturePipePointOriginUnresolved,
    FeaturePipePathTangentAssumedFalse,
    FeaturePipeLinearizeAssumedFalse,
    SurfacePatchInteriorConstraintAssumed,
    SurfacePatchUnnamedGroupsAssumedInactive,
    SurfacePatchUnorderedReferenceRunsAssumed,
    /// A box scope has no fully decoded dimensions and local frame.
    FeatureBoxPrimitiveUnresolved,
    FeatureBoxOffsetFrameAssumed,
    FeatureBoxSupportFrameAssumed,
    FeatureBoxSupportFlagsAssumed,
    /// Sheet-metal base flanges projected with their stored thickness copy because
    /// their rule states no thickness.
    SheetThicknessUnruled,
    /// Decoded feature scopes have no neutral construction-history feature.
    FeatureScopeUnprojected,
    /// A Move scope has no neutral `MoveBody` feature.
    MoveUnprojected,
    /// A Modify scope has no neutral `MoveBody` feature.
    ModifyUnprojected,
    /// Hole extent controls do not define an admitted termination.
    HoleExtentUnproven,
    /// A Hole carrier and its point-data records do not define an admitted placement.
    HolePlacementUnproven,
    HoleReferenceDimensionsNative,
    HoleParametersIndependentAssumed,
    HoleShapePartialAssumed,
    HoleExtentLeadingWordAssumed,
    /// A tapped Hole keeps its stored diameter instead of the bore of its thread size.
    HoleTappedDiameterStored,
    HoleStandardSizingAssumed,
    HoleCarrierAxisAssumed,
    /// Align operand roles and motion have no neutral binding.
    AlignOperandsUnresolved,
    /// Align has no admitted specialized prefix.
    AlignEnvelopeUnparsed,
    /// The referenced Align constructor has no admitted layout.
    AlignConstructionUnparsed,
    /// A decoded native feature envelope has no interpreted specialized payload.
    FeatureScopePayloadUninterpreted,
    /// A Fillet set list contradicts the radius group that the indexed-owner path projects.
    FeatureFilletSetConflict,
    /// A Chamfer set list pairs groups and specifications against owner order.
    FeatureChamferSetConflict,
    /// Emboss controls, native pairs, and selected contours have no neutral mapping.
    EmbossControlsNative,
    FeatureShellAuxiliaryReferencesNative,
    /// An Emboss prefix matches no admitted native construction form.
    EmbossPrefixUnparsed,
    EmbossProfileSelectionNative,
    EmbossTargetSelectionNative,
    EmbossDepthUnresolved,
    EmbossDepthSignAssumed,
    EmbossScalarOwnerAssumed,
    EmbossScalarUnitAssumed,
    EmbossPlacementNative,
    /// A non-equivalent duplicate scope envelope was superseded by the
    /// primary-indexed envelope.
    FeatureScopeEnvelopeSuperseded,
    /// Decoded Design parameters have no neutral parameter.
    ParameterUnprojected,
    /// Typed compact parameter frames match no parameter grammar.
    ParameterRecordUnparsed,
    /// Decoded parameters set byte 3 of the compact expression trailer.
    ParameterTrailerFlagUninterpreted,
    /// Decoded compact null-owner parameters have no owner binding.
    ParameterOwnerUnstored,
    /// Design parameter owner bindings have no recognized feature scope.
    ParameterOwnerUnrecognized,
    /// Design parameters retain owner frames that match no admitted grammar.
    ParameterOwnerUnparsed,
    /// Design parameters retain unit tokens without a settled quantity kind.
    ParameterUnitUntyped,
    /// Material Distance properties retain unit tags without a length conversion.
    MaterialDistanceUnitUntyped,
    /// A face material GUID names distinct visual tokens without a selection rule.
    FaceAppearanceAmbiguous,
    /// Parameter expression symbols name same-stream parameters without an edge.
    ParameterExpressionUnbound,
    /// Feature history-state dependency links were not projected.
    HistoryDependencyUnprojected,
    /// Feature history-state dependency links have multiple source scopes.
    HistoryDependencyAmbiguous,
    /// A historical face selection cannot bind uniquely to emitted input topology members.
    HistoricalFaceSelectionUnbound,
    /// A healed `DeleteFace` group assumes member order from selected face stable-slot order.
    HistoryDeleteFaceMemberOrderAssumed,
    /// Historical edge midpoint geometry could not be resolved from feature selections.
    HistoricalEdgeMidpointsUnresolved,
    /// A `Fillet` edge is inferred from blend geometry; no two supports share it.
    HistoricalFilletEdgeInferred,
    /// An edge-treatment edge is read from the faces its recipe names; it bounds no candidate face.
    HistoricalEdgeSelectionInferred,
    /// A two-face treatment group takes the edges its transition deletes; the
    /// treated set is assumed to equal the deleted set.
    HistoricalEdgeSelectionDeletedSetAssumed,
    /// A fillet with no bound history takes the one history, among several
    /// that hold its state pair, in which every edge group resolves.
    HistoricalEdgeHistoryInferred,
    /// A feature with no bound history, whose state pair several histories
    /// hold, takes the one history of the BREP blob that holds its outputs.
    HistoricalOutputBlobHistoryInferred,
    /// A `Fillet` edge is the best-ranked deleted one of several geometric candidates.
    HistoricalFilletEdgeAmbiguous,
    /// A `Fillet` edge is the one of several shared support edges along the
    /// blend axis whose span covers the blend's axial extent.
    HistoricalFilletEdgeCovering,
    /// Historical body bounding geometry could not be computed from point carriers.
    HistoricalBodyBoundsUnresolved,
    /// Historical body center geometry uses incomplete topology or vertex evidence.
    HistoricalBodyCenterPartial,
    /// A `SplitBody` feature takes its split plane from result faces and body geometry.
    HistoricalSplitPlaneInferred,
    /// Sketch relations retain native operands; no unique neutral relation.
    SketchRelationNativeRetained,
    /// Sketch dimensions retain native operands; no unique neutral dimension.
    SketchDimensionNativeRetained,
    /// Decoded Sketch placements have no neutral sketch.
    SketchPlacementUnprojected,
    /// Decoded sketch points have no neutral sketch entity.
    SketchPointUnprojected,
    /// One indexed sketch-point record failed decoding.
    SketchPointUndecoded,
    /// Decoded sketch curves have no neutral sketch entity.
    SketchCurveUnprojected,
    /// Decoded sketch surfaces have no neutral spatial sketch entity.
    SketchSurfaceUnprojected,
    /// Decoded sketch text records have no neutral sketch entity.
    SketchTextUnprojected,
    /// Sketch text retains style or orientation state absent from neutral text.
    SketchTextStyleUnprojected,
    /// Decoded sketch relations have no neutral constraint.
    SketchRelationUnprojected,
    /// Sketch patterns retain unrepresented symmetry, suppression or flag state.
    SketchPatternStateUnprojected,
    /// Sketch Geometry members set the byte-34 flag, which has no neutral sketch field.
    SketchGeometryFlagUnprojected,
    /// A spatial sketch's closed-profile search stopped at a local cap.
    SketchProfileSearchCapped,
    /// Non-construction sketch curves form no closed profile loop.
    SketchCurveUnprofiled,
    /// A sketch curve outside every closed profile loop bounds a region an Extrude selects.
    SketchCurveUnprofiledSelected,
    /// A sketch curve outside whole-entity loops bounds a closed planar region.
    SketchCurveRegionBoundary,
    /// Design dimension parameters have no parameter-backed sketch constraint.
    DimensionUnprojected,
    /// Feature profile selections retain native identities; no unique profile.
    FeatureProfileSelectionNative,
    /// Multiple Sketch-profile operands retain an unresolved group composition.
    FeatureExtrudeProfileGroupNative,
    FeatureExtrudeSurfaceProfileAssumed,
    FeatureExtrudeSymmetricDistanceAssumed,
    FeatureExtrudeWallThicknessNative,
    FeatureExtrudeAuxiliarySelectionNative,
    FeatureExtrudeFaceControlAssumed,
    FeatureExtrudeSketchPlacementNative,
    FeatureExtrudeTargetOptionAssumed,
    FeatureExtrudeStartFaceAbsentAssumed,
    FeatureExtrudeZeroSideTaperIgnored,
    /// An Extrude face profile's inferred face set lists unresolved members in face-slot order.
    FeatureExtrudeProfileFaceOrderAssumed,
    /// An Extrude face profile's end-cap search stopped at a local cap.
    FeatureExtrudeProfileFaceSearchCapped,
    /// An Extrude face profile takes the preceding faces that its transition's end caps cover.
    FeatureExtrudeProfileCapFacesInferred,
    /// An Extrude face profile takes the faces that its transition deletes on the plane of its resolved faces.
    FeatureExtrudeProfileDeletedFacesInferred,
    /// An Extrude face profile stays native because its end caps or deleted faces contradict a face set.
    FeatureExtrudeProfileFaceSetDisputed,
    FeatureSketchProfileFlagsAssumed,
    /// Extrude profile selections skipped the sketch arrangement search at a local cap.
    FeatureProfileArrangementCapped,
    /// Extrude profile selections skipped a certified profile test at a local cap.
    FeatureProfileDensificationCapped,
    /// Extrude profile selections skipped a self-crossing loop split at a local cap.
    FeatureProfileCrossingLoopCapped,
    /// Extrude profile selections split a self-crossing loop by curve-use counts alone.
    FeatureProfileCrossingLoopAssumed,
    /// Extrude profile selections matched a counted member to an edge of
    /// coincident curves by the direction of the curve it names.
    /// Extrude profile selections joined stored loops to faces through the assumed reading of their first entry operand.
    FeatureProfileLoopFlagsAssumed,
    FeatureProfileCoincidentDirectionAssumed,
    /// Extrude profile selections took regions from the faces a cut changed, at one point per region.
    FeatureProfileCutRegionsAssumed,
    /// A stored Cut transition certifies regions with stale curve identities.
    FeatureProfileRegionIdentitiesStale,
    /// Extrude profile selections assumed that a curve outside the
    /// arrangement makes no island in a selected face.
    FeatureProfileIslandAbsenceAssumed,
    /// Extrude profile selections took the polyline side of a projected point
    /// within the tolerance of a NURBS tube of a face boundary.
    FeatureProfilePolylineSideAssumed,
    /// Extrude profile selections compared part of a cap outline by samples alone.
    FeatureProfileCapOutlineAssumed,
    /// Extrude profile selections read a cap edge carrier from its samples alone.
    FeatureProfileCapCarrierAssumed,
    /// Extrude profile selections assume that a supported hole boundary of an inserted region
    /// identifies the selected profile.
    FeatureProfileHoleBoundaryInferred,
    /// Extrude profile selections assume that an inserted region's holes belong to the selected
    /// profile.
    FeatureProfileRegionHoleInferred,
    /// Feature path selections retain native identities; no unique path.
    FeaturePathSelectionNative,
    /// Feature face selections retain native candidates; no unique face.
    FeatureFaceSelectionNative,
    DeleteFaceFrameControlsAssumed,
    DeleteFaceGroupSelectionAssumed,
    SurfaceDeleteFaceWidePrologueAssumed,
    /// Legacy face operands use a current active face after no historical slot proof.
    FeatureFaceSelectionActiveSubstituted,
    /// Offset Surface support groups with different ordinals are read as one
    /// support-face selection.
    FeatureSurfaceOffsetGroupOrdinalsAssumed,
    FeatureFormCageOrderAssumed,
    FeatureFormEmptyCagesAssumed,
    /// A legacy Scale form is selected by the frame length of its English
    /// spelling; its payload length matches no form.
    FeatureScaleFrameLengthFormAssumed,
    FeatureScaleMatrixWordAssumed,
    FeatureScaleUnlistedGroupsAssumed,
    FeatureWorkAxisRuleUnassigned,
    FeatureWorkAxisEndpointToleranceAssumed,
    /// Feature body selections retain native identities; no unique body.
    FeatureBodySelectionNative,
    /// Reverse Normal body-group members lack the whole-body record type.
    FeatureReverseNormalBodyUnverified,
    FeatureFaceGroupsTypeUnverified,
    FeatureFaceGroupsPrefixUnparsed,
    FeatureFaceGroupsSourceAssumed,
    FeatureFaceGroupsSourceUnavailable,
    FeatureFaceGroupsSourceTargetUnverified,
    FeatureFaceGroupsRelatedMeshTargetUnverified,
    FeatureFaceGroupsRelatedMeshRoleUnassigned,
    FeatureFaceGroupsGroupHintRoleUnassigned,
    FeatureFaceGroupsGroupRolesAssumed,
    FeatureFaceGroupsMemberRunUnrepresentable,
    FeatureFaceGroupsMethodAssumed,
    FeatureFaceGroupsMethodUnmapped,
    FeatureFaceGroupsBoundaryDistanceAssumed,
    FeatureFaceGroupsSettingUnassigned,
    UnstitchFrameAssumed,
    UnstitchGroupSelectionAssumed,
    UnstitchPrologueControlsAssumed,
    /// Combine body refinement violates operand arity or disjoint membership.
    FeatureCombineSelectionNative,
    #[doc = "A Combine lacks a complete Boolean construction and remains typed as an unresolved Combine."]
    FeatureCombineDefinitionUnresolved,
    FeatureCombineMiddleListOmittedAssumed,
    FeatureCombineComponentDestinationAssumed,
    FeatureCombineNamedTailPrologueAssumed,
    /// A Combine frame that matches the fixed fields of the operand-list
    /// prologue is read in the standard prologue.
    FeatureCombinePrologueAssumed,
    FeatureCombineKeptToolsCutAssumed,
    /// A Combine whose lists name its operands, whose other fields contradict
    /// the operand-list prologue or cannot be checked, and which no prologue
    /// reads stays native.
    FeatureCombineOperandListRefused,
    /// A Combine with a companion `RemoveBody` scope reads every tool as
    /// removed, though the companion body selections do not name exactly the
    /// tools.
    FeatureCombineToolConsumptionAssumed,
    /// Fixed Fillet scalar lanes have no complete group assignment.
    FeatureFixedFilletAssignmentNative,
    /// Extrude scope retains an unassigned extent payload.
    FeatureExtrudeExtentNative,
    /// An Extrude face extent has fewer termination groups than its side count.
    FeatureExtrudeTerminationMissing,
    /// A marker-one `Revolve` scope has a token-independent frame length outside its documented set.
    FeatureRevolveLayoutAssumed,
    /// A marked `Move` transform record breaks a type, frame-length, or form-word rule of the rigid-transform record.
    FeatureMoveTransformAssumed,
    /// A form-2 rigid-transform record stores four references whose effect on its stored motion is unresolved.
    FeatureMoveTransformReferencesAssumed,
    FeatureMoveScopeReferencesAssumed,
    FeatureMoveFaceScopeReferencesAssumed,
    FeatureMoveFaceSelectionUnresolved,
    FeatureMoveFaceTypeAssumed,
    FeatureMoveFaceTypeConflict,
    FeatureMoveFacePrologueAssumed,
    FeatureMoveFaceMatrixAssumed,
    FeatureMoveTransformModeAssumed,
    FeatureMoveTransformPrefixAssumed,
    FeatureMoveTransformIdentityAssumed,
    PositionEnvelopeUnparsed,
    PositionSemanticsAssumed,
    PositionIdentityAssumed,
    PositionOccurrenceUnresolved,
    PositionTransformUnprojected,
    FeatureAlignMotionAssumed,
    FeatureAlignIdentityAssumed,
    FeatureAlignMotionUnprojected,
    /// A projected `Modify` assumes that its unlisted body groups select the bodies of its listed groups.
    FeatureModifyUnlistedGroupsAssumed,
    /// A projected `Modify` stores a reference table outside the body-group run of its family.
    FeatureModifyTableAssumed,
    /// A projected `Modify` commits a current history state.
    FeatureModifyStateAssumed,
    /// A standard `Loft` scope stores a non-null marked reference whose target and setting are unresolved.
    FeatureLoftReferenceUnresolved,
    /// A three-flag `Loft` scope stores flag bytes whose Loft settings are unresolved.
    FeatureLoftSettingsAssumed,
    /// A `Loft` scope body has no unique reading of its section and rail lists, and the scope stays native.
    FeatureLoftListsUnresolved,
    /// The section and rail lists of a `Loft` scope do not assign its construction-operand groups, and the scope stays native.
    FeatureLoftGroupsUnassigned,
    /// A `Loft` projects the one role-`0x7` group of its rail list as a centerline.
    FeatureLoftCenterlineAssumed,
    /// A `SurfaceExtend` scope projects with an unresolved extension method.
    FeatureSurfaceExtendMethodUnresolved,
    /// A `SurfaceExtend` scope stores a nonpositive extension distance and stays native.
    FeatureSurfaceExtendDistanceNonpositive,
    FeatureSurfaceBoundaryFlagRecordAssumed,
    FeaturePathPatternFeatureSeedAssumed,
    FeaturePathPatternReorderedSettingsAssumed,
    FeaturePathPatternSeedUnresolved,
    FeaturePathPatternPathUnresolved,
    FeaturePathPatternDefinitionPartial,
    FeaturePathPatternCountOutOfRange,
    FeaturePathPatternDistanceNonpositive,
    FeaturePathPatternStartUnresolved,
    FeaturePathPatternSettingsUnresolved,
    FeaturePathPatternPlacementNotTransferred,
    /// A rectangular pattern's symmetric distribution or coincident direction has no linear pattern field.
    FeatureRectangularPatternPlacementNotTransferred,
    /// A `Thicken` scope stores a prologue flag byte `1` whose Thicken setting is unresolved.
    FeatureThickenFlagsAssumed,
    ThickenTargetSelectionAssumed,
    ThickenFrameAssumed,
    ThickenThicknessUnresolved,
    ThickenThicknessUnrepresentable,
    ThickenFaceSelectionAssumed,
    /// A `Shell` reference table stores references outside its face-group members.
    FeatureShellTableMembersAssumed,
    FeatureShellSourceUnavailable,
    FeatureShellEnvelopeAssumed,
    FeatureShellCountedControlAssumed,
    FeatureShellSlotDirectionAssumed,
    FeatureShellSelectionUnresolved,
    FeatureShellWallValueUnresolved,
    FeatureShellGroupRolesAssumed,
    FeatureShellControlsUninterpreted,
    FeatureShellIndependentWallsNative,
    /// A compact, marked, or counted `Shell` prefix stores an offset-21 Boolean whose Shell option is unresolved.
    FeatureShellPrefixFlagAssumed,
    /// Feature face operands stay unresolved inside historical selections.
    FeatureFaceOperandUnresolved,
    /// `SurfaceTrim` cell tables outside the removal reading retain native entries.
    FeatureTrimCellTableNative,
    /// `SurfaceTrim` group members without a decoded operand record project by group role.
    FeatureTrimMemberTypeAssumed,
    /// An Extrude bounded-face operand drops fallback faces that one recipe operand contradicts.
    FeatureFaceOperandFallbackContradicted,
    /// An Extrude to-face termination keeps its native group: its recipe face and
    /// the derived preceding-face set disagree and no stored cap decides.
    FeatureExtrudeTargetDisputed,
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
    MeshGuidBytesUninterpreted,
    MeshSplitStreamAssumed,
    MeshHistoryWindowAssumed,
    MeshOperationRecordsNative,
    MeshFaceGroupsAssumed,
    MeshBodyWordsNative,
    MeshCollectionWordsNative,
    MeshBodyBytesUninterpreted,
    MeshBodyIdentityAssumed,
    MeshWrapperBytesUninterpreted,
    MeshOwnerBytesUninterpreted,
    MeshScopePrefixUninterpreted,
    /// The external-reference table was present but not decoded.
    XrefTableUndecoded,
    /// A typed occurrence placement failed its frame grammar.
    XrefPlacementUndecoded,
    /// An external reference has no admitted occurrence placement.
    XrefOccurrenceUnplaced,
    /// An external reference keeps the identity fallback without placement proof.
    XrefPlacementIdentityAssumed,
    /// Legacy placement identities remain in their complete native frames.
    XrefPlacementMetadataNative,
    /// A legacy placement role has no containing reference-table entry.
    XrefPlacementRoleUnbound,
    /// Root external occurrences take their component-path carrier pose; the
    /// placement matrix stays native.
    XrefPlacementCurrentPose,
    /// An owned external occurrence keeps its placement matrix; its
    /// component-path carrier pose stays native.
    XrefCurrentPoseNative,
    /// Two or more component-path carriers name one placement; its placement
    /// matrix stays in place.
    XrefCurrentPoseAmbiguous,
    /// Local occurrences at the document root take their component-path
    /// carrier pose; the carrier's stored placement stays native.
    LocalOccurrenceCurrentPose,
    /// Owned local occurrences keep their stored placement; their
    /// component-path carrier pose stays native.
    LocalOccurrenceCurrentPoseNative,
    /// Two or more component-path carriers name one local occurrence; its
    /// stored placement stays in place.
    LocalOccurrenceCurrentPoseAmbiguous,
    /// Nested local occurrences take the world pose of their component-path
    /// carrier; the stored local placement is replaced.
    LocalOccurrencePathPose,
    /// Component-path carriers that name no unique expanded local occurrence
    /// are not applied.
    LocalOccurrencePathPoseUnresolved,
    /// Two component-path carriers give one local occurrence different
    /// poses; the stored placement stays in place.
    LocalOccurrencePathPoseDisagrees,
    /// Component-path carriers that each name several expanded local
    /// occurrences take the one assignment to distinct occurrences under which
    /// every definition-local pose agrees. The assignment is an inference;
    /// the bytes do not store it.
    LocalOccurrencePathPoseInferred,
    /// External occurrence instances take the pose their contained
    /// occurrences' component-path carriers store; the placement matrix stays
    /// native.
    XrefContainedPose,
    /// The contained occurrences' component-path carriers of an instance give
    /// different poses; it keeps its placement.
    XrefContainedPoseDisagrees,
    /// Contained-occurrence component-path carriers name no merged instance
    /// with their owner chain.
    XrefContainedPoseNative,
    /// External occurrence instances below local owners take the pose their own
    /// component-path carriers store; the placement matrix stays native.
    XrefOwnedPose,
    /// An instance's own component-path carriers give different poses; it keeps
    /// its placement.
    XrefOwnedPoseDisagrees,
    /// Own component-path carriers that place no instance stay native.
    XrefOwnedPoseNative,
    /// A Design stream has no type and record index for placement admission.
    XrefPlacementMetadataMissing,
    /// Structured placement records were superseded by scope-bound carriers.
    XrefPlacementSuperseded,
    /// An external occurrence owner has no unique local component instance.
    XrefOwnerUnresolved,
    /// Inserted-design occurrences that store the identity and are placed by
    /// their joints; the joint-solved pose is not computed.
    JointPlacedOccurrenceUnsolved,
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
    /// Faces were omitted because their required surface reference was dangling.
    FaceSurfaceReferenceDangling,
    /// Faces rest on spline or procedural surfaces whose shape was not decoded.
    SurfaceShapeNotDecoded,
    /// Faces use zero-payload `mesh_surface` sentinels; exact surfaces are absent.
    MeshSurfaceSentinel,
    /// Sketch-region body records of a keyless BREP blob are not model bodies.
    SketchRegionBodyOmitted,
    /// Edges reference a procedural 3D curve with no decodable B-spline cache.
    ProceduralCurveUndecoded,
    /// Vertex tolerance is widened to cover exact cache endpoints.
    VertexToleranceWidened,
    /// A cache-first spring support pcurve has no admitted endpoint relation.
    SpringSupportPcurveUnresolved,
    /// A native support pcurve has no surface in its construction chart.
    ProceduralSupportChartUnresolved,
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
    /// Radial symmetry pairs have no unambiguous neutral topology correspondence.
    TsplineRadialMapUnresolved,
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
    /// A Design `BulkStream` stores a record run after its last indexed
    /// record; the run is not decoded and its bytes are retained natively.
    UnindexedRecordsRetained,
    /// Design-intent decoding failed; its sketches, features and history were dropped.
    DesignIntentUndecoded,
    /// A body presentation display name matches multiple body bindings or conflicts on one body.
    BodyDisplayNameAmbiguous,
    /// Constructed planes assign conflicting display names to one `WorkPlane` scope.
    WorkPlaneDisplayNameAmbiguous,
    /// Constructed plane display names could not be decoded.
    WorkPlaneDisplayNameUndecoded,
}

impl F3dLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [F3dLossCode] = &[
        Self::SceneOverrideRefused,
        Self::SceneOverrideAmbiguous,
        Self::SceneOverrideUnbound,
        Self::OperationPoseApplied,
        Self::ScopeOwnerUnresolved,
        Self::ScopeOutputOwnerMismatch,
        Self::ScopeOutputOwnerUnresolved,
        Self::ScopeSketchOwnerMismatch,
        Self::OperationPoseDisagrees,
        Self::OperationPoseNative,
        Self::DimensionCompanionUntyped,
        Self::BodyReferenceTailUntyped,
        Self::BodyRecipeSerializationFormAssumed,
        Self::PlacementConstructionUntyped,
        Self::WorkPlaneFrameUnbound,
        Self::WorkPlaneInferredAxisAssumed,
        Self::CylinderPlacementUnbound,
        Self::SpherePrefixFramingAssumed,
        Self::SpherePlacementFlagsAssumed,
        Self::SphereLocalMatrixFramingAssumed,
        Self::SphereLocalFrameAssumed,
        Self::SpherePlacementFrameAssumed,
        Self::SphereDiameterOwnerAssumed,
        Self::SphereDiameterSourceAssumed,
        Self::ConstructionTrackingPathUntyped,
        Self::ConstructionClosingFlagAssumed,
        Self::ChamferEdgeRoleAssumed,
        Self::ChamferDefinitionUnresolved,
        Self::ChamferSelectionUnresolved,
        Self::ChamferReferenceFaceAssumed,
        Self::ChamferAngleReferenceAssumed,
        Self::ChamferCornerTreatmentUnresolved,
        Self::ChamferTangentChainAssumed,
        Self::ChamferParameterOwnerAssumed,
        Self::ChamferParameterValueAssumed,
        Self::ChamferParameterKindAssumed,
        Self::ChamferParameterUnitAssumed,
        Self::CoilTargetMembersAssumed,
        Self::ConfigurationMemberUnassigned,
        Self::ConfigurationRuleUnbound,
        Self::ConfigurationParameterOverrideUnbound,
        Self::ConfigurationFeatureSuppressionUnbound,
        Self::ConfigurationSuppressionInferred,
        Self::ConfigurationActiveRowUnbound,
        Self::ActComponentLinkUnresolved,
        Self::LocalOccurrenceUnplaced,
        Self::LocalOccurrenceOwnerUnbound,
        Self::ComponentBodyOwnerUnresolved,
        Self::ComponentBodyOwnerFallback,
        Self::LocalOccurrenceOwnerCycle,
        Self::LocalOccurrenceExpansionLimit,
        Self::FeatureOccurrencePathUnresolved,
        Self::SketchOwnerUnresolved,
        Self::SketchNameComposedFromFeatureOrdinal,
        Self::SketchMembershipConflict,
        Self::SketchFolderClosingReference,
        Self::SketchFolderUnprojected,
        Self::SketchGeometryGroupMemberUnlisted,
        Self::SketchGeometryGroupMemberUnbacklinked,
        Self::ComponentInsertUnplaced,
        Self::ComponentInsertOrderUnknown,
        Self::ComponentInsertContainerAssumed,
        Self::ComponentDefinitionRevised,
        Self::ComponentNamingSpaceUnbound,
        Self::RootInstanceDefinitionUnbound,
        Self::LocalOccurrenceEnvelopeSuperseded,
        Self::LocalOccurrenceEnvelopeNative,
        Self::LocalComponentPathNative,
        Self::LocalOccurrenceTypeUnregistered,
        Self::LocalOccurrenceYieldedToRegistered,
        Self::LocalOccurrenceKeyRepeated,
        Self::LocalOccurrenceFirstMatrixRefused,
        Self::LocalOccurrenceIdentityOrdinalUnresolved,
        Self::LocalOccurrenceSourceUnregistered,
        Self::SavedFreeEdgeBodyUnowned,
        Self::AssemblyJointUnprojected,
        Self::AssemblyAlignmentPartial,
        Self::AssemblyRigidGroupFrameUnparsed,
        Self::AssemblyJointMotionUndecoded,
        Self::AssemblyJointRestUnprojected,
        Self::AssemblyJointActivationAssumed,
        Self::AssemblyJointAngularLimitsNative,
        Self::AssemblyOperandPathPairsNative,
        Self::AssemblyRootPathIdentitiesNative,
        Self::AssemblyOperandDescentAmbiguous,
        Self::AssemblyOperandDescentUnresolved,
        Self::AssemblyJointSourceBindingAssumed,
        Self::AssemblyConnectorOperandsAssumed,
        Self::AssemblyMotionBindingAssumed,
        Self::AssemblyAdditionalOwnerMotionAssumed,
        Self::AssemblyExtendedTailAssumed,
        Self::AssemblyExtendedTypedTailAssumed,
        Self::AssemblyOperandNestedPathRetained,
        Self::DrawingDocumentOmitted,
        Self::HistoryBindingBudgetExceeded,
        Self::HistoryBindingWorkExceeded,
        Self::RecipeSideSearchBudgetExceeded,
        Self::ParameterOrderBudgetExceeded,
        Self::HistoryRecordFramingFailed,
        Self::DesignBodyBindingUnresolved,
        Self::HistoryOnlyBodyOmitted,
        Self::ReferenceImageNativeRetained,
        Self::ReferenceImageFlagAssumed,
        Self::DecalNativeRetained,
        Self::DecalMappingAssumed,
        Self::DecalTargetUnresolved,
        Self::DecalImageUnresolved,
        Self::DecalDefinitionUnresolved,
        Self::EdgeReferenceLostUnrepaired,
        Self::FeatureDefinitionIncomplete,
        Self::FeatureMeshImportTessellationsUnresolved,
        Self::FeatureMeshImportEmptySourceAssumed,
        Self::FeatureMeshImportSourceUnparsed,
        Self::FeatureMeshSectionPlaneUnresolved,
        Self::FeatureMeshShellControlsUnresolved,
        Self::FeatureMeshOperationTailPartial,
        Self::FeatureMeshBodySelectionAssumed,
        Self::FeatureMeshOperationSourceUnresolved,
        Self::FeatureMeshOperationScopeUnparsed,
        Self::FeatureMeshOperationSelectorUnruled,
        Self::FeatureMeshOperationParameterUnbound,
        Self::FeatureMeshOperationFieldsUninterpreted,
        Self::FeatureMeshSectionCurvesUnresolved,
        Self::FeatureMeshConversionScopeUnparsed,
        Self::FeatureMeshConversionTailPartial,
        Self::FeatureMeshConversionMethodUnresolved,
        Self::FeatureMeshConversionMethodAssumed,
        Self::FeatureMeshConversionEnvelopeAssumed,
        Self::FeatureMeshConversionSourceUnresolved,
        Self::FeatureMeshConversionOutputUnresolved,
        Self::ThreadLiveFaceUnproven,
        Self::FeatureStoredBodiesEmpty,
        Self::FeatureStoredBodiesRecipeAbsent,
        Self::FeatureStoredBodiesUnbound,
        Self::FeatureDerivedInstanceComponentInsertAssumed,
        Self::FeatureComponentInsertMembershipAssumed,
        Self::FeatureDerivedInstanceSourcesUnresolved,
        Self::FeatureDerivedInstanceCarrierPlacementAssumed,
        Self::FeatureDerivedInstanceScopePlacementAssumed,
        Self::FeatureDerivedInstanceContextTransformAssumed,
        Self::FeatureDerivedInstancePlacementUnrepresentable,
        Self::FeatureDerivedInstanceDefinitionUnresolved,
        Self::FeaturePasteSourcesUnresolved,
        Self::FeatureOccurrenceTransferSourceUnresolved,
        Self::FeatureOccurrenceTransferOwnerUnresolved,
        Self::FeatureOccurrenceTransferOccurrenceUnresolved,
        Self::FeatureOccurrenceTransferPlacementUnresolved,
        Self::FeatureTimelineOwnerCompeting,
        Self::FeatureTimelineNullSlotAssumed,
        Self::FeatureTimelineOwnerUnpaired,
        Self::FeaturePasteCopyPlacementAssumed,
        Self::FeatureMirrorPlaneWorkPlaneAssumed,
        Self::FeatureCircularPatternPlacementNotTransferred,
        Self::FeatureCircularPatternDistributionAssumed,
        Self::FeatureOutputStateChainUnbound,
        Self::FeatureOutputBodyUnbound,
        Self::FeatureDerivedBaseUnresolved,
        Self::FeaturePipeCircularSectionInferred,
        Self::FeaturePipeWholePathExtentAssumed,
        Self::FeaturePipeCircularSectionAssumed,
        Self::FeaturePipePathEndSectionAssumed,
        Self::FeatureSweepSourceUnavailable,
        Self::FeatureSweepOperationPrefixAssumed,
        Self::FeatureSweepOperationUnresolved,
        Self::FeatureSweepSectionRoleAssumed,
        Self::FeatureSweepProfileCarrierAssumed,
        Self::FeatureSweepSelectionUnresolved,
        Self::FeatureSweepScalarUnresolved,
        Self::FeatureSweepTargetsNative,
        Self::FeatureSweepResultModeUnresolved,
        Self::FeatureSweepResultModeInferred,
        Self::FeatureSweepPathTangentAssumedFalse,
        Self::FeatureSweepLinearizeAssumedFalse,
        Self::FeatureSweepControlsUninterpreted,
        Self::FeaturePipeSourceUnresolved,
        Self::FeaturePipeOperationUnresolved,
        Self::FeaturePipeHollowUnresolved,
        Self::FeaturePipeScalarUnresolved,
        Self::FeaturePipeSectionUnresolved,
        Self::FeaturePipePathUnresolved,
        Self::FeaturePipePathExtentUnresolved,
        Self::FeaturePipePathParameterizationAssumed,
        Self::FeaturePipePathRoleAssumed,
        Self::FeaturePipeTargetsRetainedNative,
        Self::FeaturePipeTargetsUnresolved,
        Self::FeaturePipePointOriginUnresolved,
        Self::FeaturePipePathTangentAssumedFalse,
        Self::FeaturePipeLinearizeAssumedFalse,
        Self::SurfacePatchInteriorConstraintAssumed,
        Self::SurfacePatchUnnamedGroupsAssumedInactive,
        Self::SurfacePatchUnorderedReferenceRunsAssumed,
        Self::FeatureBoxPrimitiveUnresolved,
        Self::FeatureBoxOffsetFrameAssumed,
        Self::FeatureBoxSupportFrameAssumed,
        Self::FeatureBoxSupportFlagsAssumed,
        Self::SheetThicknessUnruled,
        Self::FeatureScopeUnprojected,
        Self::MoveUnprojected,
        Self::ModifyUnprojected,
        Self::HoleExtentUnproven,
        Self::HolePlacementUnproven,
        Self::HoleReferenceDimensionsNative,
        Self::HoleParametersIndependentAssumed,
        Self::HoleShapePartialAssumed,
        Self::HoleExtentLeadingWordAssumed,
        Self::HoleTappedDiameterStored,
        Self::HoleStandardSizingAssumed,
        Self::HoleCarrierAxisAssumed,
        Self::AlignOperandsUnresolved,
        Self::AlignEnvelopeUnparsed,
        Self::AlignConstructionUnparsed,
        Self::FeatureScopePayloadUninterpreted,
        Self::FeatureFilletSetConflict,
        Self::FeatureChamferSetConflict,
        Self::EmbossControlsNative,
        Self::FeatureShellAuxiliaryReferencesNative,
        Self::EmbossPrefixUnparsed,
        Self::EmbossProfileSelectionNative,
        Self::EmbossTargetSelectionNative,
        Self::EmbossDepthUnresolved,
        Self::EmbossDepthSignAssumed,
        Self::EmbossScalarOwnerAssumed,
        Self::EmbossScalarUnitAssumed,
        Self::EmbossPlacementNative,
        Self::FeatureScopeEnvelopeSuperseded,
        Self::ParameterUnprojected,
        Self::ParameterRecordUnparsed,
        Self::ParameterTrailerFlagUninterpreted,
        Self::ParameterOwnerUnstored,
        Self::ParameterOwnerUnrecognized,
        Self::ParameterOwnerUnparsed,
        Self::ParameterUnitUntyped,
        Self::MaterialDistanceUnitUntyped,
        Self::FaceAppearanceAmbiguous,
        Self::ParameterExpressionUnbound,
        Self::HistoryDependencyUnprojected,
        Self::HistoryDependencyAmbiguous,
        Self::HistoricalFaceSelectionUnbound,
        Self::HistoryDeleteFaceMemberOrderAssumed,
        Self::HistoricalEdgeMidpointsUnresolved,
        Self::HistoricalFilletEdgeInferred,
        Self::HistoricalEdgeSelectionInferred,
        Self::HistoricalEdgeSelectionDeletedSetAssumed,
        Self::HistoricalEdgeHistoryInferred,
        Self::HistoricalOutputBlobHistoryInferred,
        Self::HistoricalFilletEdgeAmbiguous,
        Self::HistoricalFilletEdgeCovering,
        Self::HistoricalBodyBoundsUnresolved,
        Self::HistoricalBodyCenterPartial,
        Self::HistoricalSplitPlaneInferred,
        Self::SketchRelationNativeRetained,
        Self::SketchDimensionNativeRetained,
        Self::SketchPlacementUnprojected,
        Self::SketchPointUnprojected,
        Self::SketchPointUndecoded,
        Self::SketchCurveUnprojected,
        Self::SketchSurfaceUnprojected,
        Self::SketchTextUnprojected,
        Self::SketchTextStyleUnprojected,
        Self::SketchRelationUnprojected,
        Self::SketchPatternStateUnprojected,
        Self::SketchGeometryFlagUnprojected,
        Self::SketchProfileSearchCapped,
        Self::SketchCurveUnprofiled,
        Self::SketchCurveUnprofiledSelected,
        Self::SketchCurveRegionBoundary,
        Self::DimensionUnprojected,
        Self::FeatureProfileSelectionNative,
        Self::FeatureExtrudeProfileGroupNative,
        Self::FeatureExtrudeSurfaceProfileAssumed,
        Self::FeatureExtrudeSymmetricDistanceAssumed,
        Self::FeatureExtrudeWallThicknessNative,
        Self::FeatureExtrudeAuxiliarySelectionNative,
        Self::FeatureExtrudeFaceControlAssumed,
        Self::FeatureExtrudeSketchPlacementNative,
        Self::FeatureExtrudeTargetOptionAssumed,
        Self::FeatureExtrudeStartFaceAbsentAssumed,
        Self::FeatureExtrudeZeroSideTaperIgnored,
        Self::FeatureExtrudeProfileFaceOrderAssumed,
        Self::FeatureExtrudeProfileFaceSearchCapped,
        Self::FeatureExtrudeProfileCapFacesInferred,
        Self::FeatureExtrudeProfileDeletedFacesInferred,
        Self::FeatureExtrudeProfileFaceSetDisputed,
        Self::FeatureSketchProfileFlagsAssumed,
        Self::FeatureProfileArrangementCapped,
        Self::FeatureProfileDensificationCapped,
        Self::FeatureProfileCrossingLoopCapped,
        Self::FeatureProfileCrossingLoopAssumed,
        Self::FeatureProfileCoincidentDirectionAssumed,
        Self::FeatureProfileLoopFlagsAssumed,
        Self::FeatureProfileCutRegionsAssumed,
        Self::FeatureProfileRegionIdentitiesStale,
        Self::FeatureProfileIslandAbsenceAssumed,
        Self::FeatureProfilePolylineSideAssumed,
        Self::FeatureProfileCapOutlineAssumed,
        Self::FeatureProfileCapCarrierAssumed,
        Self::FeatureProfileHoleBoundaryInferred,
        Self::FeatureProfileRegionHoleInferred,
        Self::FeaturePathSelectionNative,
        Self::FeatureFaceSelectionNative,
        Self::DeleteFaceFrameControlsAssumed,
        Self::DeleteFaceGroupSelectionAssumed,
        Self::SurfaceDeleteFaceWidePrologueAssumed,
        Self::FeatureFaceSelectionActiveSubstituted,
        Self::FeatureSurfaceOffsetGroupOrdinalsAssumed,
        Self::FeatureFormCageOrderAssumed,
        Self::FeatureFormEmptyCagesAssumed,
        Self::FeatureScaleFrameLengthFormAssumed,
        Self::FeatureScaleMatrixWordAssumed,
        Self::FeatureScaleUnlistedGroupsAssumed,
        Self::FeatureWorkAxisRuleUnassigned,
        Self::FeatureWorkAxisEndpointToleranceAssumed,
        Self::FeatureBodySelectionNative,
        Self::FeatureReverseNormalBodyUnverified,
        Self::FeatureFaceGroupsTypeUnverified,
        Self::FeatureFaceGroupsPrefixUnparsed,
        Self::FeatureFaceGroupsSourceAssumed,
        Self::FeatureFaceGroupsSourceUnavailable,
        Self::FeatureFaceGroupsSourceTargetUnverified,
        Self::FeatureFaceGroupsRelatedMeshTargetUnverified,
        Self::FeatureFaceGroupsRelatedMeshRoleUnassigned,
        Self::FeatureFaceGroupsGroupHintRoleUnassigned,
        Self::FeatureFaceGroupsGroupRolesAssumed,
        Self::FeatureFaceGroupsMemberRunUnrepresentable,
        Self::FeatureFaceGroupsMethodAssumed,
        Self::FeatureFaceGroupsMethodUnmapped,
        Self::FeatureFaceGroupsBoundaryDistanceAssumed,
        Self::FeatureFaceGroupsSettingUnassigned,
        Self::UnstitchFrameAssumed,
        Self::UnstitchGroupSelectionAssumed,
        Self::UnstitchPrologueControlsAssumed,
        Self::FeatureCombineSelectionNative,
        Self::FeatureCombineDefinitionUnresolved,
        Self::FeatureCombineMiddleListOmittedAssumed,
        Self::FeatureCombineComponentDestinationAssumed,
        Self::FeatureCombineNamedTailPrologueAssumed,
        Self::FeatureCombinePrologueAssumed,
        Self::FeatureCombineKeptToolsCutAssumed,
        Self::FeatureCombineOperandListRefused,
        Self::FeatureCombineToolConsumptionAssumed,
        Self::FeatureFixedFilletAssignmentNative,
        Self::FeatureExtrudeExtentNative,
        Self::FeatureExtrudeTerminationMissing,
        Self::FeatureRevolveLayoutAssumed,
        Self::FeatureMoveTransformAssumed,
        Self::FeatureMoveTransformReferencesAssumed,
        Self::FeatureMoveScopeReferencesAssumed,
        Self::FeatureMoveFaceScopeReferencesAssumed,
        Self::FeatureMoveFaceSelectionUnresolved,
        Self::FeatureMoveFaceTypeAssumed,
        Self::FeatureMoveFaceTypeConflict,
        Self::FeatureMoveFacePrologueAssumed,
        Self::FeatureMoveFaceMatrixAssumed,
        Self::FeatureMoveTransformModeAssumed,
        Self::FeatureMoveTransformPrefixAssumed,
        Self::FeatureMoveTransformIdentityAssumed,
        Self::PositionEnvelopeUnparsed,
        Self::PositionSemanticsAssumed,
        Self::PositionIdentityAssumed,
        Self::PositionOccurrenceUnresolved,
        Self::PositionTransformUnprojected,
        Self::FeatureAlignMotionAssumed,
        Self::FeatureAlignIdentityAssumed,
        Self::FeatureAlignMotionUnprojected,
        Self::FeatureModifyUnlistedGroupsAssumed,
        Self::FeatureModifyTableAssumed,
        Self::FeatureModifyStateAssumed,
        Self::FeatureLoftReferenceUnresolved,
        Self::FeatureLoftSettingsAssumed,
        Self::FeatureLoftListsUnresolved,
        Self::FeatureLoftGroupsUnassigned,
        Self::FeatureLoftCenterlineAssumed,
        Self::FeatureSurfaceExtendMethodUnresolved,
        Self::FeatureSurfaceExtendDistanceNonpositive,
        Self::FeatureSurfaceBoundaryFlagRecordAssumed,
        Self::FeaturePathPatternFeatureSeedAssumed,
        Self::FeaturePathPatternReorderedSettingsAssumed,
        Self::FeaturePathPatternSeedUnresolved,
        Self::FeaturePathPatternPathUnresolved,
        Self::FeaturePathPatternDefinitionPartial,
        Self::FeaturePathPatternCountOutOfRange,
        Self::FeaturePathPatternDistanceNonpositive,
        Self::FeaturePathPatternStartUnresolved,
        Self::FeaturePathPatternSettingsUnresolved,
        Self::FeaturePathPatternPlacementNotTransferred,
        Self::FeatureRectangularPatternPlacementNotTransferred,
        Self::FeatureThickenFlagsAssumed,
        Self::ThickenTargetSelectionAssumed,
        Self::ThickenFrameAssumed,
        Self::ThickenThicknessUnresolved,
        Self::ThickenThicknessUnrepresentable,
        Self::ThickenFaceSelectionAssumed,
        Self::FeatureShellTableMembersAssumed,
        Self::FeatureShellSourceUnavailable,
        Self::FeatureShellEnvelopeAssumed,
        Self::FeatureShellCountedControlAssumed,
        Self::FeatureShellSlotDirectionAssumed,
        Self::FeatureShellSelectionUnresolved,
        Self::FeatureShellWallValueUnresolved,
        Self::FeatureShellGroupRolesAssumed,
        Self::FeatureShellControlsUninterpreted,
        Self::FeatureShellIndependentWallsNative,
        Self::FeatureShellPrefixFlagAssumed,
        Self::FeatureFaceOperandUnresolved,
        Self::FeatureTrimCellTableNative,
        Self::FeatureTrimMemberTypeAssumed,
        Self::FeatureFaceOperandFallbackContradicted,
        Self::FeatureExtrudeTargetDisputed,
        Self::FeatureEdgeSelectionNative,
        Self::FeatureEdgeOperandUnresolved,
        Self::FeatureEdgeSelectionLost,
        Self::FeaturePatternInstanceSearchCapped,
        Self::BrepBlobUndecoded,
        Self::MeshContainerUnjoined,
        Self::MeshContainerUndecoded,
        Self::MeshContainerMissing,
        Self::MeshAttributeNotTransferred,
        Self::MeshGuidBytesUninterpreted,
        Self::MeshSplitStreamAssumed,
        Self::MeshHistoryWindowAssumed,
        Self::MeshOperationRecordsNative,
        Self::MeshFaceGroupsAssumed,
        Self::MeshBodyWordsNative,
        Self::MeshCollectionWordsNative,
        Self::MeshBodyBytesUninterpreted,
        Self::MeshBodyIdentityAssumed,
        Self::MeshWrapperBytesUninterpreted,
        Self::MeshOwnerBytesUninterpreted,
        Self::MeshScopePrefixUninterpreted,
        Self::XrefTableUndecoded,
        Self::XrefPlacementUndecoded,
        Self::XrefOccurrenceUnplaced,
        Self::XrefPlacementIdentityAssumed,
        Self::XrefPlacementMetadataNative,
        Self::XrefPlacementRoleUnbound,
        Self::XrefPlacementCurrentPose,
        Self::XrefCurrentPoseNative,
        Self::XrefCurrentPoseAmbiguous,
        Self::LocalOccurrenceCurrentPose,
        Self::LocalOccurrenceCurrentPoseNative,
        Self::LocalOccurrenceCurrentPoseAmbiguous,
        Self::LocalOccurrencePathPose,
        Self::LocalOccurrencePathPoseUnresolved,
        Self::LocalOccurrencePathPoseDisagrees,
        Self::LocalOccurrencePathPoseInferred,
        Self::XrefContainedPose,
        Self::XrefContainedPoseDisagrees,
        Self::XrefContainedPoseNative,
        Self::XrefOwnedPose,
        Self::XrefOwnedPoseDisagrees,
        Self::XrefOwnedPoseNative,
        Self::XrefPlacementMetadataMissing,
        Self::XrefPlacementSuperseded,
        Self::XrefOwnerUnresolved,
        Self::JointPlacedOccurrenceUnsolved,
        Self::MeshVertexPrecisionReduced,
        Self::BodylessDesignCarrier,
        Self::AssemblyComponentsExternal,
        Self::NurbsSurfaceCarrier,
        Self::SketchRegionBodyOmitted,
        Self::NurbsCurveCarrier,
        Self::FaceSurfaceReferenceDangling,
        Self::SurfaceShapeNotDecoded,
        Self::MeshSurfaceSentinel,
        Self::ProceduralCurveUndecoded,
        Self::VertexToleranceWidened,
        Self::SpringSupportPcurveUnresolved,
        Self::ProceduralSupportChartUnresolved,
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
        Self::TsplineRadialMapUnresolved,
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
        Self::UnindexedRecordsRetained,
        Self::DesignIntentUndecoded,
        Self::BodyDisplayNameAmbiguous,
        Self::WorkPlaneDisplayNameAmbiguous,
        Self::WorkPlaneDisplayNameUndecoded,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::SceneOverrideRefused => "scene.override-refused",
            Self::UnfoldAllBendsFlagAssumed => "feature.unfold-all-bends-flag-assumed",
            Self::UnfoldControlUnresolved => "feature.unfold-control-unresolved",
            Self::UnfoldSelectionSlotsAssumed => "feature.unfold-selection-slots-assumed",
            Self::UnfoldSelectionNative => "feature.unfold-selection-native",
            Self::UnfoldEnvelopeUnresolved => "feature.unfold-envelope-unresolved",
            Self::UnfoldMaterialScalarFactorAssumed => {
                "feature.unfold-material-scalar-factor-assumed"
            }
            Self::UnfoldRuleFactorUnresolved => "feature.unfold-rule-factor-unresolved",
            Self::UnfoldSettingsRuleReferenceAssumed => {
                "feature.unfold-settings-rule-reference-assumed"
            }
            Self::RefoldGroupSourceAssumed => "feature.refold-group-source-assumed",
            Self::RefoldSourceUnresolved => "feature.refold-source-unresolved",
            Self::BendLineOwnerOrderAssumed => "feature.bend-line-owner-order-assumed",
            Self::BendStationaryRoleAssumed => "feature.bend-stationary-role-assumed",
            Self::BendAngleUnresolved => "feature.bend-angle-unresolved",
            Self::BendRadiusFirstAssumed => "feature.bend-radius-first-assumed",
            Self::BendStationarySelectionNative => "feature.bend-stationary-selection-native",
            Self::BendRuleRadiusUnresolved => "feature.bend-rule-radius-unresolved",
            Self::BendAngleOwnerBacklinkAssumed => "feature.bend-angle-owner-backlink-assumed",
            Self::BendCachedOwnerAngleAssumed => "feature.bend-cached-owner-angle-assumed",
            Self::SceneOverrideAmbiguous => "scene.override-ambiguous",
            Self::SceneOverrideUnbound => "scene.override-unbound",
            Self::ScopeOwnerUnresolved => "scope.owner-unresolved",
            Self::ScopeOutputOwnerMismatch => "scope.output-owner-mismatch",
            Self::ScopeOutputOwnerUnresolved => "scope.output-owner-unresolved",
            Self::ScopeSketchOwnerMismatch => "scope.sketch-owner-mismatch",
            Self::OperationPoseApplied => "assembly.operation-table-pose",
            Self::OperationPoseDisagrees => "assembly.operation-table-pose-disagrees",
            Self::OperationPoseNative => "assembly.operation-table-pose-native",
            Self::DimensionCompanionUntyped => "dimension.companion-untyped",
            Self::BodyReferenceTailUntyped => "body-reference.tail-untyped",
            Self::BodyRecipeSerializationFormAssumed => "body-recipe.serialization-form-assumed",
            Self::PlacementConstructionUntyped => "placement.construction-untyped",
            Self::WorkPlaneFrameUnbound => "work-plane.frame-unbound",
            Self::WorkPlaneInferredAxisAssumed => "work-plane.inferred-axis-assumed",
            Self::CylinderPlacementUnbound => "feature.cylinder-placement-unbound",
            Self::SpherePrefixFramingAssumed => "feature.sphere-prefix-framing-assumed",
            Self::SpherePlacementFlagsAssumed => "feature.sphere-placement-flags-assumed",
            Self::SphereLocalMatrixFramingAssumed => "feature.sphere-local-matrix-framing-assumed",
            Self::SphereLocalFrameAssumed => "feature.sphere-local-frame-assumed",
            Self::SpherePlacementFrameAssumed => "feature.sphere-placement-frame-assumed",
            Self::SphereDiameterOwnerAssumed => "feature.sphere-diameter-owner-assumed",
            Self::SphereDiameterSourceAssumed => "feature.sphere-diameter-source-assumed",

            Self::ConstructionTrackingPathUntyped => "feature.tracking-path-untyped",
            Self::ConstructionClosingFlagAssumed => "feature.construction-closing-flag-assumed",
            Self::ChamferEdgeRoleAssumed => "feature.chamfer-edge-role-assumed",
            Self::ChamferDefinitionUnresolved => "feature.chamfer-definition-unresolved",
            Self::ChamferSelectionUnresolved => "feature.chamfer-selection-unresolved",
            Self::ChamferReferenceFaceAssumed => "feature.chamfer-reference-face-assumed",
            Self::ChamferAngleReferenceAssumed => "feature.chamfer-angle-reference-assumed",
            Self::ChamferCornerTreatmentUnresolved => "feature.chamfer-corner-treatment-unresolved",
            Self::ChamferTangentChainAssumed => "feature.chamfer-tangent-chain-assumed",
            Self::ChamferParameterOwnerAssumed => "feature.chamfer-parameter-owner-assumed",
            Self::ChamferParameterValueAssumed => "feature.chamfer-parameter-value-assumed",
            Self::ChamferParameterKindAssumed => "feature.chamfer-parameter-kind-assumed",
            Self::ChamferParameterUnitAssumed => "feature.chamfer-parameter-unit-assumed",
            Self::CoilTargetMembersAssumed => "feature.coil-target-members-assumed",
            Self::ConfigurationMemberUnassigned => "configuration.member-unassigned",
            Self::ConfigurationRuleUnbound => "configuration.rule-unbound",
            Self::ConfigurationParameterOverrideUnbound => {
                "configuration.parameter-override-unbound"
            }
            Self::ConfigurationFeatureSuppressionUnbound => {
                "configuration.feature-suppression-unbound"
            }
            Self::ConfigurationSuppressionInferred => "configuration.suppression-inferred",
            Self::ConfigurationActiveRowUnbound => "configuration.active-row-unbound",
            Self::ActComponentLinkUnresolved => "assembly.act-component-link-unresolved",
            Self::LocalOccurrenceUnplaced => "assembly.local-occurrence-unplaced",
            Self::LocalOccurrenceOwnerUnbound => "assembly.local-occurrence-owner-unbound",
            Self::ComponentBodyOwnerFallback => "assembly.component-body-owner-fallback",
            Self::ComponentBodyOwnerUnresolved => "assembly.component-body-owner-unresolved",
            Self::LocalOccurrenceOwnerCycle => "assembly.local-occurrence-owner-cycle",
            Self::LocalOccurrenceExpansionLimit => "assembly.local-occurrence-expansion-limit",
            Self::FeatureOccurrencePathUnresolved => "feature.occurrence-path-unresolved",
            Self::SketchOwnerUnresolved => "sketch.component-owner-unresolved",
            Self::SketchNameComposedFromFeatureOrdinal => {
                "sketch.name-composed-from-feature-ordinal"
            }
            Self::SketchMembershipConflict => "sketch.membership-conflict",
            Self::SketchFolderClosingReference => "sketch.folder-closing-reference",
            Self::SketchFolderUnprojected => "sketch.folder-unprojected",
            Self::SketchGeometryGroupMemberUnlisted => "sketch.geometry-group-member-unlisted",
            Self::SketchGeometryGroupMemberUnbacklinked => {
                "sketch.geometry-group-member-unbacklinked"
            }
            Self::ComponentInsertUnplaced => "assembly.component-insert-unplaced",
            Self::ComponentInsertOrderUnknown => "assembly.component-insert-order-unknown",
            Self::ComponentInsertContainerAssumed => "assembly.component-insert-container-assumed",
            Self::ComponentDefinitionRevised => "assembly.component-definition-revised",
            Self::ComponentNamingSpaceUnbound => "assembly.component-naming-space-unbound",
            Self::RootInstanceDefinitionUnbound => "assembly.root-instance-definition-unbound",
            Self::SavedFreeEdgeBodyUnowned => "assembly.saved-free-edge-body-unowned",
            Self::LocalOccurrenceEnvelopeNative => "assembly.local-occurrence-envelope-native",
            Self::LocalComponentPathNative => "assembly.local-component-path-native",
            Self::LocalOccurrenceIdentityOrdinalUnresolved => {
                "assembly.local-occurrence-identity-ordinal-unresolved"
            }
            Self::LocalOccurrenceSourceUnregistered => {
                "assembly.local-occurrence-source-unregistered"
            }
            Self::LocalOccurrenceEnvelopeSuperseded => {
                "assembly.local-occurrence-envelope-superseded"
            }
            Self::LocalOccurrenceTypeUnregistered => "assembly.local-occurrence-type-unregistered",
            Self::LocalOccurrenceYieldedToRegistered => {
                "assembly.local-occurrence-yielded-to-registered"
            }
            Self::LocalOccurrenceKeyRepeated => "assembly.local-occurrence-key-repeated",
            Self::LocalOccurrenceFirstMatrixRefused => {
                "assembly.local-occurrence-first-matrix-refused"
            }
            Self::AssemblyAlignmentPartial => "assembly.alignment-partial",
            Self::AssemblyJointUnprojected => "assembly.joint-unprojected",
            Self::AssemblyRigidGroupFrameUnparsed => "assembly.rigid-group-frame-unparsed",
            Self::AssemblyJointMotionUndecoded => "assembly.joint-motion-undecoded",
            Self::AssemblyJointRestUnprojected => "assembly.joint-rest-unprojected",
            Self::AssemblyJointActivationAssumed => "assembly.joint-activation-assumed",
            Self::AssemblyJointAngularLimitsNative => "assembly.joint-angular-limits-native",
            Self::AssemblyOperandPathPairsNative => "assembly.operand-path-pairs-native",
            Self::AssemblyRootPathIdentitiesNative => "assembly.root-path-identities-native",
            Self::AssemblyOperandDescentAmbiguous => "assembly.operand-descent-ambiguous",
            Self::AssemblyOperandDescentUnresolved => "assembly.operand-descent-unresolved",
            Self::AssemblyJointSourceBindingAssumed => "assembly.joint-source-binding-assumed",
            Self::AssemblyConnectorOperandsAssumed => "assembly.connector-operands-assumed",
            Self::AssemblyMotionBindingAssumed => "assembly.motion-binding-assumed",
            Self::AssemblyAdditionalOwnerMotionAssumed => {
                "assembly.additional-owner-motion-assumed"
            }
            Self::AssemblyExtendedTailAssumed => "assembly.extended-tail-assumed",
            Self::AssemblyExtendedTypedTailAssumed => "assembly.extended-typed-tail-assumed",
            Self::AssemblyOperandNestedPathRetained => "assembly.operand-nested-path-retained",
            Self::DrawingDocumentOmitted => "drawing.document-omitted",
            Self::HistoryBindingBudgetExceeded => "history.binding-budget-exceeded",
            Self::HistoryBindingWorkExceeded => "history.binding-work-exceeded",
            Self::RecipeSideSearchBudgetExceeded => "feature.recipe-search-budget-exceeded",
            Self::ParameterOrderBudgetExceeded => "parameter.order-budget-exceeded",
            Self::HistoryRecordFramingFailed => "history.record-framing-failed",
            Self::DesignBodyBindingUnresolved => "design.body-binding-unresolved",
            Self::HistoryOnlyBodyOmitted => "design.history-only-body-omitted",
            Self::ReferenceImageNativeRetained => "appearance.reference-image-native",
            Self::ReferenceImageFlagAssumed => "appearance.reference-image-flag-assumed",
            Self::DecalNativeRetained => "appearance.decal-native",
            Self::DecalMappingAssumed => "appearance.decal-mapping-assumed",
            Self::DecalTargetUnresolved => "appearance.decal-target-unresolved",
            Self::DecalImageUnresolved => "appearance.decal-image-unresolved",
            Self::DecalDefinitionUnresolved => "appearance.decal-definition-unresolved",
            Self::EdgeReferenceLostUnrepaired => "feature.edge-reference-lost",
            Self::FeatureStoredBodiesEmpty => "feature.stored-bodies-empty",
            Self::FeatureStoredBodiesRecipeAbsent => "feature.stored-bodies-recipe-absent",
            Self::FeatureStoredBodiesUnbound => "feature.stored-bodies-unbound",
            Self::FeatureDerivedInstanceComponentInsertAssumed => {
                "feature.derived-instance-component-insert-assumed"
            }
            Self::FeatureComponentInsertMembershipAssumed => {
                "feature.component-insert-membership-assumed"
            }
            Self::FeatureDerivedInstanceSourcesUnresolved => {
                "feature.derived-instance-sources-unresolved"
            }
            Self::FeatureDerivedInstanceCarrierPlacementAssumed => {
                "feature.derived-instance-carrier-placement-assumed"
            }
            Self::FeatureDerivedInstanceScopePlacementAssumed => {
                "feature.derived-instance-scope-placement-assumed"
            }
            Self::FeatureDerivedInstanceContextTransformAssumed => {
                "feature.derived-instance-context-transform-assumed"
            }
            Self::FeatureDerivedInstancePlacementUnrepresentable => {
                "feature.derived-instance-placement-unrepresentable"
            }
            Self::FeatureDerivedInstanceDefinitionUnresolved => {
                "feature.derived-instance-definition-unresolved"
            }
            Self::FeaturePasteSourcesUnresolved => "feature.paste-sources-unresolved",
            Self::FeatureOccurrenceTransferSourceUnresolved => {
                "feature.occurrence-transfer-source-unresolved"
            }
            Self::FeatureOccurrenceTransferOwnerUnresolved => {
                "feature.occurrence-transfer-owner-unresolved"
            }
            Self::FeatureOccurrenceTransferOccurrenceUnresolved => {
                "feature.occurrence-transfer-occurrence-unresolved"
            }
            Self::FeatureOccurrenceTransferPlacementUnresolved => {
                "feature.occurrence-transfer-placement-unresolved"
            }
            Self::FeatureTimelineOwnerCompeting => "feature.timeline-owner-competing",
            Self::FeatureTimelineNullSlotAssumed => "feature.timeline-null-slot-assumed",
            Self::FeatureTimelineOwnerUnpaired => "feature.timeline-owner-unpaired",
            Self::FeaturePasteCopyPlacementAssumed => "feature.paste-copy-placement-assumed",
            Self::FeatureMirrorPlaneWorkPlaneAssumed => "feature.mirror-plane-work-plane-assumed",
            Self::FeatureCircularPatternPlacementNotTransferred => {
                "feature.circular-pattern-placement-not-transferred"
            }
            Self::FeatureCircularPatternDistributionAssumed => {
                "feature.circular-pattern-distribution-assumed"
            }
            Self::FeatureOutputStateChainUnbound => "feature.output-state-chain-unbound",
            Self::FeatureOutputBodyUnbound => "feature.output-body-unbound",
            Self::FeatureDerivedBaseUnresolved => "feature.derived-base-unresolved",
            Self::FeaturePipeCircularSectionInferred => "feature.pipe-circular-section-inferred",
            Self::FeaturePipeWholePathExtentAssumed => "feature.pipe-whole-path-extent-assumed",
            Self::FeaturePipeCircularSectionAssumed => "feature.pipe-circular-section-assumed",
            Self::FeaturePipePathEndSectionAssumed => "feature.pipe-path-end-section-assumed",
            Self::FeatureSweepSourceUnavailable => "feature.sweep-source-unavailable",
            Self::FeatureSweepOperationPrefixAssumed => "feature.sweep-operation-prefix-assumed",
            Self::FeatureSweepOperationUnresolved => "feature.sweep-operation-unresolved",
            Self::FeatureSweepSectionRoleAssumed => "feature.sweep-section-role-assumed",
            Self::FeatureSweepProfileCarrierAssumed => "feature.sweep-profile-carrier-assumed",
            Self::FeatureSweepSelectionUnresolved => "feature.sweep-selection-unresolved",
            Self::FeatureSweepScalarUnresolved => "feature.sweep-scalar-unresolved",
            Self::FeatureSweepTargetsNative => "feature.sweep-targets-native",
            Self::FeatureSweepResultModeUnresolved => "feature.sweep-result-mode-unresolved",
            Self::FeatureSweepResultModeInferred => "feature.sweep-result-mode-inferred",
            Self::FeatureSweepPathTangentAssumedFalse => "feature.sweep-path-tangent-assumed-false",
            Self::FeatureSweepLinearizeAssumedFalse => "feature.sweep-linearize-assumed-false",
            Self::FeatureSweepControlsUninterpreted => "feature.sweep-controls-uninterpreted",
            Self::FeaturePipeSourceUnresolved => "feature.pipe-source-unresolved",
            Self::FeaturePipeOperationUnresolved => "feature.pipe-operation-unresolved",
            Self::FeaturePipeHollowUnresolved => "feature.pipe-hollow-unresolved",
            Self::FeaturePipeScalarUnresolved => "feature.pipe-scalar-unresolved",
            Self::FeaturePipeSectionUnresolved => "feature.pipe-section-unresolved",
            Self::FeaturePipePathUnresolved => "feature.pipe-path-unresolved",
            Self::FeaturePipePathExtentUnresolved => "feature.pipe-path-extent-unresolved",
            Self::FeaturePipePathParameterizationAssumed => {
                "feature.pipe-path-parameterization-assumed"
            }
            Self::FeaturePipePathRoleAssumed => "feature.pipe-path-role-assumed",
            Self::FeaturePipeTargetsRetainedNative => "feature.pipe-targets-retained-native",
            Self::FeaturePipeTargetsUnresolved => "feature.pipe-targets-unresolved",
            Self::FeaturePipePointOriginUnresolved => "feature.pipe-point-origin-unresolved",
            Self::FeaturePipePathTangentAssumedFalse => "feature.pipe-path-tangent-assumed-false",
            Self::FeaturePipeLinearizeAssumedFalse => "feature.pipe-linearize-assumed-false",
            Self::SurfacePatchInteriorConstraintAssumed => {
                "feature.surface-patch-interior-constraint-assumed"
            }
            Self::SurfacePatchUnnamedGroupsAssumedInactive => {
                "feature.surface-patch-unnamed-groups-assumed-inactive"
            }
            Self::SurfacePatchUnorderedReferenceRunsAssumed => {
                "feature.surface-patch-unordered-reference-runs-assumed"
            }
            Self::FeatureBoxPrimitiveUnresolved => "feature.box-primitive-unresolved",
            Self::FeatureBoxOffsetFrameAssumed => "feature.box-offset-frame-assumed",
            Self::FeatureBoxSupportFrameAssumed => "feature.box-support-frame-assumed",
            Self::FeatureBoxSupportFlagsAssumed => "feature.box-support-flags-assumed",
            Self::FeatureDefinitionIncomplete => "feature.definition-incomplete",
            Self::FeatureMeshImportTessellationsUnresolved => {
                "feature.mesh-import-tessellations-unresolved"
            }
            Self::FeatureMeshImportEmptySourceAssumed => "feature.mesh-import-empty-source-assumed",
            Self::FeatureMeshImportSourceUnparsed => "feature.mesh-import-source-unparsed",
            Self::FeatureMeshSectionPlaneUnresolved => "feature.mesh-section-plane-unresolved",
            Self::FeatureMeshShellControlsUnresolved => "feature.mesh-shell-controls-unresolved",
            Self::FeatureMeshOperationTailPartial => "feature.mesh-operation-tail-partial",
            Self::FeatureMeshBodySelectionAssumed => "feature.mesh-body-selection-assumed",
            Self::FeatureMeshOperationSourceUnresolved => {
                "feature.mesh-operation-source-unresolved"
            }
            Self::FeatureMeshOperationScopeUnparsed => "feature.mesh-operation-scope-unparsed",
            Self::FeatureMeshOperationSelectorUnruled => "feature.mesh-operation-selector-unruled",
            Self::FeatureMeshOperationParameterUnbound => {
                "feature.mesh-operation-parameter-unbound"
            }
            Self::FeatureMeshOperationFieldsUninterpreted => {
                "feature.mesh-operation-fields-uninterpreted"
            }
            Self::FeatureMeshSectionCurvesUnresolved => "feature.mesh-section-curves-unresolved",
            Self::FeatureMeshConversionScopeUnparsed => "feature.mesh-conversion-scope-unparsed",
            Self::FeatureMeshConversionTailPartial => "feature.mesh-conversion-tail-partial",
            Self::FeatureMeshConversionMethodUnresolved => {
                "feature.mesh-conversion-method-unresolved"
            }
            Self::FeatureMeshConversionMethodAssumed => "feature.mesh-conversion-method-assumed",
            Self::FeatureMeshConversionEnvelopeAssumed => {
                "feature.mesh-conversion-envelope-assumed"
            }
            Self::FeatureMeshConversionSourceUnresolved => {
                "feature.mesh-conversion-source-unresolved"
            }
            Self::FeatureMeshConversionOutputUnresolved => {
                "feature.mesh-conversion-output-unresolved"
            }
            Self::ThreadLiveFaceUnproven => "feature.thread-live-face-unproven",
            Self::SheetThicknessUnruled => "feature.sheet-thickness-unruled",
            Self::EmbossControlsNative => "feature.emboss-controls-native",
            Self::FeatureShellAuxiliaryReferencesNative => {
                "feature.shell-auxiliary-references-native"
            }
            Self::EmbossPrefixUnparsed => "feature.emboss-prefix-unparsed",
            Self::EmbossProfileSelectionNative => "feature.emboss-profile-selection-native",
            Self::EmbossTargetSelectionNative => "feature.emboss-target-selection-native",
            Self::EmbossDepthUnresolved => "feature.emboss-depth-unresolved",
            Self::EmbossDepthSignAssumed => "feature.emboss-depth-sign-assumed",
            Self::EmbossScalarOwnerAssumed => "feature.emboss-scalar-owner-assumed",
            Self::EmbossScalarUnitAssumed => "feature.emboss-scalar-unit-assumed",
            Self::EmbossPlacementNative => "feature.emboss-placement-native",
            Self::AlignOperandsUnresolved => "feature.align-operands-unresolved",
            Self::AlignEnvelopeUnparsed => "feature.align-envelope-unparsed",
            Self::AlignConstructionUnparsed => "feature.align-construction-unparsed",
            Self::FeatureScopePayloadUninterpreted => "feature.scope-payload-uninterpreted",
            Self::FeatureFilletSetConflict => "feature.fillet-set-conflict",
            Self::FeatureChamferSetConflict => "feature.chamfer-set-conflict",
            Self::FeatureScopeEnvelopeSuperseded => "feature.scope-envelope-superseded",
            Self::HoleExtentUnproven => "feature.hole-extent-unproven",
            Self::HolePlacementUnproven => "feature.hole-placement-unproven",
            Self::HoleReferenceDimensionsNative => "feature.hole-reference-dimensions-native",
            Self::HoleParametersIndependentAssumed => "feature.hole-parameters-independent-assumed",
            Self::HoleShapePartialAssumed => "feature.hole-shape-partial-assumed",
            Self::HoleExtentLeadingWordAssumed => "feature.hole-extent-leading-word-assumed",
            Self::HoleTappedDiameterStored => "feature.hole-tapped-diameter-stored",
            Self::HoleStandardSizingAssumed => "feature.hole-standard-sizing-assumed",
            Self::HoleCarrierAxisAssumed => "feature.hole-carrier-axis-assumed",
            Self::FeatureScopeUnprojected => "feature.scope-unprojected",
            Self::MoveUnprojected => "feature.move-unprojected",
            Self::ModifyUnprojected => "feature.modify-unprojected",
            Self::ParameterUnprojected => "parameter.unprojected",
            Self::ParameterRecordUnparsed => "parameter.record-unparsed",
            Self::ParameterTrailerFlagUninterpreted => "parameter.trailer-flag-uninterpreted",
            Self::ParameterOwnerUnstored => "parameter.owner-unstored",
            Self::ParameterOwnerUnrecognized => "parameter.owner-unrecognized",
            Self::ParameterOwnerUnparsed => "parameter.owner-unparsed",
            Self::ParameterUnitUntyped => "parameter.unit-untyped",
            Self::MaterialDistanceUnitUntyped => "material.distance-unit-untyped",
            Self::FaceAppearanceAmbiguous => "material.face-appearance-ambiguous",
            Self::ParameterExpressionUnbound => "parameter.expression-unbound",
            Self::HistoryDependencyUnprojected => "history.dependency-unprojected",
            Self::HistoricalFaceSelectionUnbound => "history.face-selection-unbound",
            Self::HistoryDeleteFaceMemberOrderAssumed => "history.delete-face-member-order-assumed",
            Self::HistoricalEdgeMidpointsUnresolved => "history.edge-midpoints-unresolved",
            Self::HistoricalFilletEdgeInferred => "history.fillet-edge-inferred",
            Self::HistoricalEdgeSelectionInferred => "history.edge-selection-inferred",
            Self::HistoricalEdgeSelectionDeletedSetAssumed => {
                "history.edge-selection-deleted-set-assumed"
            }
            Self::HistoricalEdgeHistoryInferred => "history.edge-history-inferred",
            Self::HistoricalOutputBlobHistoryInferred => "history.output-blob-history-inferred",
            Self::HistoricalFilletEdgeAmbiguous => "history.fillet-edge-ambiguous",
            Self::HistoricalFilletEdgeCovering => "history.fillet-edge-covering",
            Self::HistoricalBodyBoundsUnresolved => "history.body-bounds-unresolved",
            Self::HistoricalBodyCenterPartial => "history.body-center-partial",
            Self::HistoricalSplitPlaneInferred => "history.split-plane-inferred",
            Self::HistoryDependencyAmbiguous => "history.dependency-ambiguous",
            Self::SketchRelationNativeRetained => "sketch.relation-native",
            Self::SketchDimensionNativeRetained => "sketch.dimension-native",
            Self::SketchPlacementUnprojected => "sketch.placement-unprojected",
            Self::SketchPointUnprojected => "sketch.point-unprojected",
            Self::SketchPointUndecoded => "sketch.point-undecoded",
            Self::SketchCurveUnprojected => "sketch.curve-unprojected",
            Self::SketchSurfaceUnprojected => "sketch.surface-unprojected",
            Self::SketchTextUnprojected => "sketch.text-unprojected",
            Self::SketchTextStyleUnprojected => "sketch.text-style-unprojected",
            Self::SketchRelationUnprojected => "sketch.relation-unprojected",
            Self::SketchPatternStateUnprojected => "sketch.pattern-state-unprojected",
            Self::SketchGeometryFlagUnprojected => "sketch.geometry-flag-unprojected",
            Self::SketchProfileSearchCapped => "sketch.profile-search-capped",
            Self::SketchCurveUnprofiled => "sketch.curve-unprofiled",
            Self::SketchCurveUnprofiledSelected => "sketch.curve-unprofiled-selected",
            Self::SketchCurveRegionBoundary => "sketch.curve-region-boundary",
            Self::DimensionUnprojected => "dimension.unprojected",
            Self::FeatureSketchProfileFlagsAssumed => "feature.sketch-profile-flags-assumed",
            Self::FeatureExtrudeProfileGroupNative => "feature.extrude-profile-group-native",
            Self::FeatureExtrudeSurfaceProfileAssumed => "feature.extrude-surface-profile-assumed",
            Self::FeatureExtrudeSymmetricDistanceAssumed => {
                "feature.extrude-symmetric-distance-assumed"
            }
            Self::FeatureExtrudeWallThicknessNative => "feature.extrude-wall-thickness-native",
            Self::FeatureExtrudeAuxiliarySelectionNative => {
                "feature.extrude-auxiliary-selection-native"
            }
            Self::FeatureExtrudeFaceControlAssumed => "feature.extrude-face-control-assumed",
            Self::FeatureExtrudeSketchPlacementNative => "feature.extrude-sketch-placement-native",
            Self::FeatureExtrudeTargetOptionAssumed => "feature.extrude-target-option-assumed",
            Self::FeatureExtrudeStartFaceAbsentAssumed => {
                "feature.extrude-start-face-absent-assumed"
            }
            Self::FeatureExtrudeZeroSideTaperIgnored => "feature.extrude-zero-side-taper-ignored",
            Self::FeatureExtrudeProfileFaceOrderAssumed => {
                "feature.extrude-profile-face-order-assumed"
            }
            Self::FeatureExtrudeProfileFaceSearchCapped => {
                "feature.extrude-profile-face-search-capped"
            }
            Self::FeatureExtrudeProfileCapFacesInferred => {
                "feature.extrude-profile-cap-faces-inferred"
            }
            Self::FeatureExtrudeProfileDeletedFacesInferred => {
                "feature.extrude-profile-deleted-faces-inferred"
            }
            Self::FeatureExtrudeProfileFaceSetDisputed => {
                "feature.extrude-profile-face-set-disputed"
            }
            Self::FeatureProfileSelectionNative => "feature.profile-selection-native",
            Self::FeatureProfileArrangementCapped => "feature.profile-arrangement-capped",
            Self::FeatureProfileDensificationCapped => "feature.profile-densification-capped",
            Self::FeatureProfileCrossingLoopCapped => "feature.profile-crossing-loop-capped",
            Self::FeatureProfileCrossingLoopAssumed => "feature.profile-crossing-loop-assumed",
            Self::FeatureProfileLoopFlagsAssumed => "feature.profile-loop-flags-assumed",
            Self::FeatureProfileCoincidentDirectionAssumed => {
                "feature.profile-coincident-direction-assumed"
            }
            Self::FeatureProfileRegionIdentitiesStale => "feature.profile-region-identities-stale",
            Self::FeatureProfileCutRegionsAssumed => "feature.profile-cut-regions-assumed",
            Self::FeatureProfileIslandAbsenceAssumed => "feature.profile-island-absence-assumed",
            Self::FeatureProfilePolylineSideAssumed => "feature.profile-polyline-side-assumed",
            Self::FeatureProfileCapOutlineAssumed => "feature.profile-cap-outline-assumed",
            Self::FeatureProfileCapCarrierAssumed => "feature.profile-cap-carrier-assumed",
            Self::FeatureProfileHoleBoundaryInferred => "feature.profile-hole-boundary-inferred",
            Self::FeatureProfileRegionHoleInferred => "feature.profile-region-hole-inferred",
            Self::FeaturePathSelectionNative => "feature.path-selection-native",
            Self::FeatureFaceSelectionNative => "feature.face-selection-native",
            Self::DeleteFaceFrameControlsAssumed => "feature.delete-face-frame-controls-assumed",
            Self::DeleteFaceGroupSelectionAssumed => "feature.delete-face-group-selection-assumed",
            Self::SurfaceDeleteFaceWidePrologueAssumed => {
                "feature.surface-delete-face-wide-prologue-assumed"
            }
            Self::FeatureFaceSelectionActiveSubstituted => {
                "feature.face-selection-active-substituted"
            }
            Self::FeatureSurfaceOffsetGroupOrdinalsAssumed => {
                "feature.surface-offset-group-ordinals-assumed"
            }
            Self::FeatureFormCageOrderAssumed => "feature.form-cage-order-assumed",
            Self::FeatureFormEmptyCagesAssumed => "feature.form-empty-cages-assumed",
            Self::FeatureScaleFrameLengthFormAssumed => "feature.scale-frame-length-form-assumed",
            Self::FeatureScaleMatrixWordAssumed => "feature.scale-matrix-word-assumed",
            Self::FeatureScaleUnlistedGroupsAssumed => "feature.scale-unlisted-groups-assumed",
            Self::FeatureWorkAxisRuleUnassigned => "feature.work-axis-rule-unassigned",
            Self::FeatureWorkAxisEndpointToleranceAssumed => {
                "feature.work-axis-endpoint-tolerance-assumed"
            }
            Self::FeatureBodySelectionNative => "feature.body-selection-native",
            Self::FeatureReverseNormalBodyUnverified => "feature.reverse-normal-body-unverified",
            Self::FeatureFaceGroupsTypeUnverified => "feature.mesh-face-groups-type-unverified",
            Self::FeatureFaceGroupsPrefixUnparsed => "feature.mesh-face-groups-prefix-unparsed",
            Self::FeatureFaceGroupsSourceAssumed => "feature.mesh-face-groups-source-assumed",
            Self::FeatureFaceGroupsSourceUnavailable => {
                "feature.mesh-face-groups-source-unavailable"
            }
            Self::FeatureFaceGroupsSourceTargetUnverified => {
                "feature.mesh-face-groups-source-target-unverified"
            }
            Self::FeatureFaceGroupsRelatedMeshTargetUnverified => {
                "feature.mesh-face-groups-related-target-unverified"
            }
            Self::FeatureFaceGroupsRelatedMeshRoleUnassigned => {
                "feature.mesh-face-groups-related-role-unassigned"
            }
            Self::FeatureFaceGroupsGroupHintRoleUnassigned => {
                "feature.mesh-face-groups-group-hint-role-unassigned"
            }
            Self::FeatureFaceGroupsGroupRolesAssumed => {
                "feature.mesh-face-groups-group-roles-assumed"
            }
            Self::FeatureFaceGroupsMemberRunUnrepresentable => {
                "feature.mesh-face-groups-member-run-unrepresentable"
            }
            Self::FeatureFaceGroupsMethodAssumed => "feature.mesh-face-groups-method-assumed",
            Self::FeatureFaceGroupsMethodUnmapped => "feature.mesh-face-groups-method-unmapped",
            Self::FeatureFaceGroupsBoundaryDistanceAssumed => {
                "feature.mesh-face-groups-boundary-distance-assumed"
            }
            Self::FeatureFaceGroupsSettingUnassigned => {
                "feature.mesh-face-groups-setting-unassigned"
            }
            Self::UnstitchFrameAssumed => "feature.unstitch-frame-assumed",
            Self::UnstitchGroupSelectionAssumed => "feature.unstitch-group-selection-assumed",
            Self::UnstitchPrologueControlsAssumed => "feature.unstitch-prologue-controls-assumed",
            Self::FeatureCombineSelectionNative => "feature.combine-selection-native",
            Self::FeatureCombineDefinitionUnresolved => "feature.combine-definition-unresolved",
            Self::FeatureCombineMiddleListOmittedAssumed => {
                "feature.combine-middle-list-omitted-assumed"
            }
            Self::FeatureCombineComponentDestinationAssumed => {
                "feature.combine-component-destination-assumed"
            }
            Self::FeatureCombineNamedTailPrologueAssumed => {
                "feature.combine-named-tail-prologue-assumed"
            }
            Self::FeatureCombinePrologueAssumed => "feature.combine-prologue-assumed",
            Self::FeatureCombineKeptToolsCutAssumed => "feature.combine-kept-tools-cut-assumed",
            Self::FeatureCombineOperandListRefused => "feature.combine-operand-list-refused",
            Self::FeatureCombineToolConsumptionAssumed => {
                "feature.combine-tool-consumption-assumed"
            }
            Self::FeatureFixedFilletAssignmentNative => "feature.fixed-fillet-assignment-native",
            Self::FeatureExtrudeExtentNative => "feature.extrude-extent-native",
            Self::FeatureExtrudeTerminationMissing => "feature.extrude-termination-missing",
            Self::FeatureRevolveLayoutAssumed => "feature.revolve-layout-assumed",
            Self::FeatureMoveTransformAssumed => "feature.move-transform-assumed",
            Self::FeatureMoveTransformReferencesAssumed => {
                "feature.move-transform-references-assumed"
            }
            Self::FeatureMoveTransformIdentityAssumed => "feature.move-transform-identity-assumed",
            Self::PositionEnvelopeUnparsed => "feature.position-envelope-unparsed",
            Self::PositionSemanticsAssumed => "feature.position-semantics-assumed",
            Self::PositionIdentityAssumed => "feature.position-identity-assumed",
            Self::PositionOccurrenceUnresolved => "feature.position-occurrence-unresolved",
            Self::PositionTransformUnprojected => "feature.position-transform-unprojected",
            Self::FeatureAlignMotionAssumed => "feature.align-motion-assumed",
            Self::FeatureAlignIdentityAssumed => "feature.align-identity-assumed",
            Self::FeatureAlignMotionUnprojected => "feature.align-motion-unprojected",
            Self::FeatureMoveTransformPrefixAssumed => "feature.move-transform-prefix-assumed",
            Self::FeatureMoveTransformModeAssumed => "feature.move-transform-mode-assumed",
            Self::FeatureMoveScopeReferencesAssumed => "feature.move-scope-references-assumed",
            Self::FeatureMoveFaceScopeReferencesAssumed => {
                "feature.move-face-scope-references-assumed"
            }
            Self::FeatureMoveFaceSelectionUnresolved => "feature.move-face-selection-unresolved",
            Self::FeatureMoveFaceTypeAssumed => "feature.move-face-type-assumed",
            Self::FeatureMoveFaceTypeConflict => "feature.move-face-type-conflict",
            Self::FeatureMoveFacePrologueAssumed => "feature.move-face-prologue-assumed",
            Self::FeatureMoveFaceMatrixAssumed => "feature.move-face-matrix-assumed",
            Self::FeatureModifyUnlistedGroupsAssumed => "feature.modify-unlisted-groups-assumed",
            Self::FeatureModifyTableAssumed => "feature.modify-table-assumed",
            Self::FeatureModifyStateAssumed => "feature.modify-state-assumed",
            Self::FeatureLoftReferenceUnresolved => "feature.loft-reference-unresolved",
            Self::FeatureLoftSettingsAssumed => "feature.loft-settings-assumed",
            Self::FeatureLoftListsUnresolved => "feature.loft-lists-unresolved",
            Self::FeatureLoftGroupsUnassigned => "feature.loft-groups-unassigned",
            Self::FeatureLoftCenterlineAssumed => "feature.loft-centerline-assumed",
            Self::FeatureSurfaceExtendMethodUnresolved => {
                "feature.surface-extend-method-unresolved"
            }
            Self::FeatureSurfaceExtendDistanceNonpositive => {
                "feature.surface-extend-distance-nonpositive"
            }
            Self::FeatureSurfaceBoundaryFlagRecordAssumed => {
                "feature.surface-boundary-flag-record-assumed"
            }
            Self::FeaturePathPatternFeatureSeedAssumed => {
                "feature.path-pattern-feature-seed-assumed"
            }
            Self::FeaturePathPatternReorderedSettingsAssumed => {
                "feature.path-pattern-reordered-settings-assumed"
            }
            Self::FeaturePathPatternSeedUnresolved => "feature.path-pattern-seed-unresolved",
            Self::FeaturePathPatternPathUnresolved => "feature.path-pattern-path-unresolved",
            Self::FeaturePathPatternDefinitionPartial => "feature.path-pattern-definition-partial",
            Self::FeaturePathPatternCountOutOfRange => "feature.path-pattern-count-out-of-range",
            Self::FeaturePathPatternDistanceNonpositive => {
                "feature.path-pattern-distance-nonpositive"
            }
            Self::FeaturePathPatternStartUnresolved => "feature.path-pattern-start-unresolved",
            Self::FeaturePathPatternSettingsUnresolved => {
                "feature.path-pattern-settings-unresolved"
            }
            Self::FeaturePathPatternPlacementNotTransferred => {
                "feature.path-pattern-placement-not-transferred"
            }
            Self::FeatureRectangularPatternPlacementNotTransferred => {
                "feature.rectangular-pattern-placement-not-transferred"
            }
            Self::FeatureThickenFlagsAssumed => "feature.thicken-flags-assumed",
            Self::ThickenTargetSelectionAssumed => "feature.thicken-target-selection-assumed",
            Self::ThickenFrameAssumed => "feature.thicken-frame-assumed",
            Self::ThickenThicknessUnresolved => "feature.thicken-thickness-unresolved",
            Self::ThickenThicknessUnrepresentable => "feature.thicken-thickness-unrepresentable",
            Self::ThickenFaceSelectionAssumed => "feature.thicken-face-selection-assumed",
            Self::FeatureShellTableMembersAssumed => "feature.shell-table-members-assumed",
            Self::FeatureShellSourceUnavailable => "feature.shell-source-unavailable",
            Self::FeatureShellEnvelopeAssumed => "feature.shell-envelope-assumed",
            Self::FeatureShellCountedControlAssumed => "feature.shell-counted-control-assumed",
            Self::FeatureShellSlotDirectionAssumed => "feature.shell-slot-direction-assumed",
            Self::FeatureShellSelectionUnresolved => "feature.shell-selection-unresolved",
            Self::FeatureShellWallValueUnresolved => "feature.shell-wall-value-unresolved",
            Self::FeatureShellGroupRolesAssumed => "feature.shell-group-roles-assumed",
            Self::FeatureShellControlsUninterpreted => "feature.shell-controls-uninterpreted",
            Self::FeatureShellIndependentWallsNative => "feature.shell-independent-walls-native",
            Self::FeatureShellPrefixFlagAssumed => "feature.shell-prefix-flag-assumed",
            Self::FeatureFaceOperandUnresolved => "feature.face-operand-unresolved",
            Self::FeatureTrimCellTableNative => "feature.trim-cell-table-native",
            Self::FeatureTrimMemberTypeAssumed => "feature.trim-member-type-assumed",
            Self::FeatureExtrudeTargetDisputed => "feature.extrude-target-disputed",
            Self::FeatureFaceOperandFallbackContradicted => {
                "feature.face-operand-fallback-contradicted"
            }
            Self::FeatureEdgeSelectionNative => "feature.edge-selection-native",
            Self::FeatureEdgeOperandUnresolved => "feature.edge-operand-unresolved",
            Self::FeatureEdgeSelectionLost => "feature.edge-selection-lost",
            Self::FeaturePatternInstanceSearchCapped => "feature.pattern-instance-search-capped",
            Self::BrepBlobUndecoded => "geometry.brep-blob-undecoded",
            Self::MeshContainerUnjoined => "mesh.container-unjoined",
            Self::MeshContainerUndecoded => "mesh.container-undecoded",
            Self::MeshContainerMissing => "mesh.container-missing",
            Self::MeshAttributeNotTransferred => "mesh.attribute-not-transferred",
            Self::MeshGuidBytesUninterpreted => "mesh.guid-bytes-uninterpreted",
            Self::MeshSplitStreamAssumed => "mesh.split-stream-assumed",
            Self::MeshHistoryWindowAssumed => "mesh.history-window-assumed",
            Self::MeshOperationRecordsNative => "mesh.operation-records-native",
            Self::MeshFaceGroupsAssumed => "mesh.face-groups-assumed",
            Self::MeshBodyWordsNative => "mesh.body-words-native",
            Self::MeshCollectionWordsNative => "mesh.collection-words-native",
            Self::MeshBodyBytesUninterpreted => "mesh.body-bytes-uninterpreted",
            Self::MeshBodyIdentityAssumed => "mesh.body-identity-assumed",
            Self::MeshWrapperBytesUninterpreted => "mesh.wrapper-bytes-uninterpreted",
            Self::MeshOwnerBytesUninterpreted => "mesh.owner-bytes-uninterpreted",
            Self::MeshScopePrefixUninterpreted => "mesh.scope-prefix-uninterpreted",
            Self::XrefTableUndecoded => "xref.table-undecoded",
            Self::XrefPlacementUndecoded => "xref.placement-undecoded",
            Self::XrefOccurrenceUnplaced => "xref.occurrence-unplaced",
            Self::XrefPlacementIdentityAssumed => "xref.placement-identity-assumed",
            Self::XrefPlacementMetadataNative => "xref.placement-metadata-native",
            Self::XrefPlacementRoleUnbound => "xref.placement-role-unbound",
            Self::XrefPlacementCurrentPose => "xref.placement-current-pose",
            Self::XrefCurrentPoseNative => "xref.current-pose-native",
            Self::XrefCurrentPoseAmbiguous => "xref.current-pose-ambiguous",
            Self::LocalOccurrenceCurrentPose => "assembly.local-occurrence-current-pose",
            Self::LocalOccurrenceCurrentPoseNative => {
                "assembly.local-occurrence-current-pose-native"
            }
            Self::LocalOccurrenceCurrentPoseAmbiguous => {
                "assembly.local-occurrence-current-pose-ambiguous"
            }
            Self::LocalOccurrencePathPose => "assembly.local-occurrence-path-pose",
            Self::LocalOccurrencePathPoseUnresolved => {
                "assembly.local-occurrence-path-pose-unresolved"
            }
            Self::LocalOccurrencePathPoseDisagrees => {
                "assembly.local-occurrence-path-pose-disagrees"
            }
            Self::LocalOccurrencePathPoseInferred => "assembly.local-occurrence-path-pose-inferred",
            Self::XrefContainedPose => "xref.contained-pose",
            Self::XrefContainedPoseDisagrees => "xref.contained-pose-disagrees",
            Self::XrefContainedPoseNative => "xref.contained-pose-native",
            Self::XrefOwnedPose => "xref.owned-pose",
            Self::XrefOwnedPoseDisagrees => "xref.owned-pose-disagrees",
            Self::XrefOwnedPoseNative => "xref.owned-pose-native",
            Self::XrefPlacementMetadataMissing => "xref.placement-metadata-missing",
            Self::XrefPlacementSuperseded => "xref.placement-superseded",
            Self::XrefOwnerUnresolved => "xref.owner-unresolved",
            Self::JointPlacedOccurrenceUnsolved => "assembly.joint-placed-occurrence-unsolved",
            Self::MeshVertexPrecisionReduced => "mesh.vertex-precision-reduced",
            Self::BodylessDesignCarrier => "design.bodyless-carrier",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::NurbsSurfaceCarrier => "geometry.nurbs-surface-carrier",
            Self::SketchRegionBodyOmitted => "geometry.sketch-region-body-omitted",
            Self::NurbsCurveCarrier => "geometry.nurbs-curve-carrier",
            Self::FaceSurfaceReferenceDangling => "geometry.face-surface-dangling",
            Self::SurfaceShapeNotDecoded => "geometry.surface-shape-not-decoded",
            Self::MeshSurfaceSentinel => "geometry.mesh-surface-sentinel",
            Self::ProceduralCurveUndecoded => "geometry.procedural-curve-undecoded",
            Self::VertexToleranceWidened => "geometry.vertex-tolerance-widened",
            Self::ProceduralSupportChartUnresolved => {
                "geometry.procedural-support-chart-unresolved"
            }
            Self::SpringSupportPcurveUnresolved => "geometry.spring-support-pcurve-unresolved",
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
            Self::TsplineRadialMapUnresolved => "tspline.radial-map-unresolved",
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
            Self::UnindexedRecordsRetained => "source.unindexed-records-retained",
            Self::DesignIntentUndecoded => "design.intent-undecoded",
            Self::BodyDisplayNameAmbiguous => "design.body-display-name-ambiguous",
            Self::WorkPlaneDisplayNameAmbiguous => "work-plane.display-name-ambiguous",
            Self::WorkPlaneDisplayNameUndecoded => "work-plane.display-name-undecoded",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::SceneOverrideRefused => Severity::Warning,
            Self::BendStationaryRoleAssumed
            | Self::BendAngleUnresolved
            | Self::BendRadiusFirstAssumed
            | Self::BendLineOwnerOrderAssumed
            | Self::BendStationarySelectionNative
            | Self::BendRuleRadiusUnresolved
            | Self::BendAngleOwnerBacklinkAssumed
            | Self::BendCachedOwnerAngleAssumed
            | Self::UnfoldAllBendsFlagAssumed
            | Self::UnfoldControlUnresolved
            | Self::UnfoldSelectionSlotsAssumed
            | Self::UnfoldSelectionNative
            | Self::UnfoldEnvelopeUnresolved
            | Self::UnfoldMaterialScalarFactorAssumed
            | Self::UnfoldRuleFactorUnresolved
            | Self::UnfoldSettingsRuleReferenceAssumed
            | Self::RefoldGroupSourceAssumed
            | Self::RefoldSourceUnresolved => Severity::Warning,
            Self::SceneOverrideAmbiguous | Self::SceneOverrideUnbound => Severity::Info,
            Self::ScopeOutputOwnerMismatch | Self::ScopeSketchOwnerMismatch => Severity::Warning,
            Self::WriterAppearancePropertiesOmitted => Severity::Warning,
            Self::ScopeOwnerUnresolved
            | Self::ScopeOutputOwnerUnresolved
            | Self::OperationPoseApplied
            | Self::OperationPoseNative
            | Self::BodylessDesignCarrier
            | Self::XrefPlacementMetadataNative
            | Self::XrefPlacementCurrentPose
            | Self::XrefCurrentPoseNative
            | Self::LocalOccurrenceCurrentPose
            | Self::LocalOccurrenceCurrentPoseNative
            | Self::XrefContainedPose
            | Self::XrefContainedPoseNative
            | Self::XrefOwnedPose
            | Self::XrefOwnedPoseNative
            | Self::AssemblyComponentsExternal
            | Self::NurbsSurfaceCarrier
            | Self::NurbsCurveCarrier
            | Self::MeshSurfaceSentinel
            | Self::MeshGuidBytesUninterpreted
            | Self::MeshBodyBytesUninterpreted
            | Self::MeshWrapperBytesUninterpreted
            | Self::MeshOwnerBytesUninterpreted
            | Self::MeshScopePrefixUninterpreted
            | Self::SketchRegionBodyOmitted
            | Self::SketchCurveRegionBoundary
            | Self::SketchCurveUnprofiledSelected
            | Self::ThreadLiveFaceUnproven
            | Self::ReferenceImageFlagAssumed
            | Self::DecalMappingAssumed => Severity::Info,
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
            Self::OperationPoseDisagrees
            | Self::DimensionCompanionUntyped
            | Self::BodyReferenceTailUntyped
            | Self::WorkPlaneFrameUnbound
            | Self::WorkPlaneInferredAxisAssumed
            | Self::CylinderPlacementUnbound
            | Self::SpherePrefixFramingAssumed
            | Self::SpherePlacementFlagsAssumed
            | Self::SphereLocalMatrixFramingAssumed
            | Self::SphereLocalFrameAssumed
            | Self::SpherePlacementFrameAssumed
            | Self::SphereDiameterOwnerAssumed
            | Self::SphereDiameterSourceAssumed
            | Self::PlacementConstructionUntyped
            | Self::ConstructionTrackingPathUntyped
            | Self::ConstructionClosingFlagAssumed
            | Self::ChamferEdgeRoleAssumed
            | Self::ChamferDefinitionUnresolved
            | Self::ChamferSelectionUnresolved
            | Self::ChamferReferenceFaceAssumed
            | Self::ChamferAngleReferenceAssumed
            | Self::ChamferCornerTreatmentUnresolved
            | Self::ChamferTangentChainAssumed
            | Self::ChamferParameterOwnerAssumed
            | Self::ChamferParameterValueAssumed
            | Self::ChamferParameterKindAssumed
            | Self::ChamferParameterUnitAssumed
            | Self::CoilTargetMembersAssumed
            | Self::ConfigurationMemberUnassigned
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::ConfigurationSuppressionInferred
            | Self::ConfigurationActiveRowUnbound
            | Self::ActComponentLinkUnresolved
            | Self::ComponentBodyOwnerUnresolved
            | Self::ComponentBodyOwnerFallback
            | Self::LocalOccurrenceOwnerUnbound
            | Self::LocalOccurrenceOwnerCycle
            | Self::LocalOccurrenceExpansionLimit
            | Self::FeatureOccurrencePathUnresolved
            | Self::LocalOccurrenceUnplaced
            | Self::SketchOwnerUnresolved
            | Self::SketchNameComposedFromFeatureOrdinal
            | Self::SketchMembershipConflict
            | Self::SketchFolderClosingReference
            | Self::SketchFolderUnprojected
            | Self::SketchGeometryGroupMemberUnlisted
            | Self::SketchGeometryGroupMemberUnbacklinked
            | Self::ComponentInsertUnplaced
            | Self::ComponentInsertOrderUnknown
            | Self::ComponentInsertContainerAssumed
            | Self::SavedFreeEdgeBodyUnowned
            | Self::LocalOccurrenceIdentityOrdinalUnresolved
            | Self::LocalOccurrenceSourceUnregistered
            | Self::LocalOccurrenceKeyRepeated
            | Self::LocalOccurrenceFirstMatrixRefused
            | Self::LocalOccurrenceEnvelopeNative
            | Self::LocalComponentPathNative
            | Self::LocalOccurrenceEnvelopeSuperseded
            | Self::LocalOccurrenceTypeUnregistered
            | Self::LocalOccurrenceYieldedToRegistered
            | Self::RootInstanceDefinitionUnbound
            | Self::ComponentDefinitionRevised
            | Self::ComponentNamingSpaceUnbound
            | Self::AssemblyAlignmentPartial
            | Self::AssemblyJointUnprojected
            | Self::AssemblyRigidGroupFrameUnparsed
            | Self::AssemblyJointMotionUndecoded
            | Self::AssemblyJointAngularLimitsNative
            | Self::AssemblyJointRestUnprojected
            | Self::AssemblyJointActivationAssumed
            | Self::AssemblyOperandPathPairsNative
            | Self::AssemblyOperandDescentUnresolved
            | Self::AssemblyJointSourceBindingAssumed
            | Self::AssemblyConnectorOperandsAssumed
            | Self::AssemblyMotionBindingAssumed
            | Self::AssemblyAdditionalOwnerMotionAssumed
            | Self::AssemblyExtendedTailAssumed
            | Self::AssemblyExtendedTypedTailAssumed
            | Self::AssemblyOperandNestedPathRetained
            | Self::AssemblyOperandDescentAmbiguous
            | Self::AssemblyRootPathIdentitiesNative
            | Self::DrawingDocumentOmitted
            | Self::DesignBodyBindingUnresolved
            | Self::HistoryOnlyBodyOmitted
            | Self::ReferenceImageNativeRetained
            | Self::DecalNativeRetained
            | Self::DecalTargetUnresolved
            | Self::DecalImageUnresolved
            | Self::DecalDefinitionUnresolved
            | Self::EdgeReferenceLostUnrepaired
            | Self::FeatureStoredBodiesEmpty
            | Self::FeatureStoredBodiesRecipeAbsent
            | Self::FeatureOutputBodyUnbound
            | Self::FeatureOutputStateChainUnbound
            | Self::FeatureStoredBodiesUnbound
            | Self::FeatureDerivedInstanceComponentInsertAssumed
            | Self::FeatureComponentInsertMembershipAssumed
            | Self::FeatureDerivedInstanceSourcesUnresolved
            | Self::FeatureDerivedInstanceCarrierPlacementAssumed
            | Self::FeatureDerivedInstanceScopePlacementAssumed
            | Self::FeatureDerivedInstanceContextTransformAssumed
            | Self::FeatureDerivedInstancePlacementUnrepresentable
            | Self::FeatureDerivedInstanceDefinitionUnresolved
            | Self::FeaturePasteSourcesUnresolved
            | Self::FeatureOccurrenceTransferSourceUnresolved
            | Self::FeatureOccurrenceTransferOwnerUnresolved
            | Self::FeatureOccurrenceTransferOccurrenceUnresolved
            | Self::FeatureOccurrenceTransferPlacementUnresolved
            | Self::FeatureTimelineOwnerCompeting
            | Self::FeatureTimelineNullSlotAssumed
            | Self::FeatureTimelineOwnerUnpaired
            | Self::FeaturePasteCopyPlacementAssumed
            | Self::FeatureMirrorPlaneWorkPlaneAssumed
            | Self::FeatureCircularPatternPlacementNotTransferred
            | Self::FeatureCircularPatternDistributionAssumed
            | Self::FeatureDerivedBaseUnresolved
            | Self::FeaturePipeCircularSectionInferred
            | Self::FeaturePipeWholePathExtentAssumed
            | Self::FeaturePipeCircularSectionAssumed
            | Self::FeaturePipePathEndSectionAssumed
            | Self::FeatureSweepSourceUnavailable
            | Self::FeatureSweepOperationPrefixAssumed
            | Self::FeatureSweepOperationUnresolved
            | Self::FeatureSweepSectionRoleAssumed
            | Self::FeatureSweepProfileCarrierAssumed
            | Self::FeatureSweepSelectionUnresolved
            | Self::FeatureSweepScalarUnresolved
            | Self::FeatureSweepTargetsNative
            | Self::FeatureSweepResultModeUnresolved
            | Self::FeatureSweepResultModeInferred
            | Self::FeatureSweepPathTangentAssumedFalse
            | Self::FeatureSweepLinearizeAssumedFalse
            | Self::FeatureSweepControlsUninterpreted
            | Self::FeaturePipeSourceUnresolved
            | Self::FeaturePipeOperationUnresolved
            | Self::FeaturePipeHollowUnresolved
            | Self::FeaturePipeScalarUnresolved
            | Self::FeaturePipeSectionUnresolved
            | Self::FeaturePipePathUnresolved
            | Self::FeaturePipePathExtentUnresolved
            | Self::FeaturePipePathParameterizationAssumed
            | Self::FeaturePipePathRoleAssumed
            | Self::FeaturePipeTargetsRetainedNative
            | Self::FeaturePipeTargetsUnresolved
            | Self::FeaturePipePointOriginUnresolved
            | Self::FeaturePipePathTangentAssumedFalse
            | Self::FeaturePipeLinearizeAssumedFalse
            | Self::SurfacePatchInteriorConstraintAssumed
            | Self::SurfacePatchUnnamedGroupsAssumedInactive
            | Self::SurfacePatchUnorderedReferenceRunsAssumed
            | Self::FeatureBoxPrimitiveUnresolved
            | Self::FeatureBoxOffsetFrameAssumed
            | Self::FeatureBoxSupportFrameAssumed
            | Self::FeatureBoxSupportFlagsAssumed
            | Self::FeatureDefinitionIncomplete
            | Self::FeatureMeshImportTessellationsUnresolved
            | Self::FeatureMeshImportEmptySourceAssumed
            | Self::FeatureMeshImportSourceUnparsed
            | Self::FeatureMeshSectionPlaneUnresolved
            | Self::FeatureMeshShellControlsUnresolved
            | Self::FeatureMeshOperationTailPartial
            | Self::FeatureMeshBodySelectionAssumed
            | Self::FeatureMeshOperationSourceUnresolved
            | Self::FeatureMeshOperationScopeUnparsed
            | Self::FeatureMeshOperationSelectorUnruled
            | Self::FeatureMeshOperationParameterUnbound
            | Self::FeatureMeshOperationFieldsUninterpreted
            | Self::FeatureMeshSectionCurvesUnresolved
            | Self::FeatureMeshConversionScopeUnparsed
            | Self::FeatureMeshConversionTailPartial
            | Self::FeatureMeshConversionMethodUnresolved
            | Self::FeatureMeshConversionMethodAssumed
            | Self::FeatureMeshConversionEnvelopeAssumed
            | Self::FeatureMeshConversionSourceUnresolved
            | Self::FeatureMeshConversionOutputUnresolved
            | Self::SheetThicknessUnruled
            | Self::HoleExtentUnproven
            | Self::HolePlacementUnproven
            | Self::HoleReferenceDimensionsNative
            | Self::HoleParametersIndependentAssumed
            | Self::HoleShapePartialAssumed
            | Self::HoleExtentLeadingWordAssumed
            | Self::HoleTappedDiameterStored
            | Self::HoleStandardSizingAssumed
            | Self::HoleCarrierAxisAssumed
            | Self::FeatureScopeUnprojected
            | Self::EmbossControlsNative
            | Self::FeatureShellAuxiliaryReferencesNative
            | Self::EmbossPrefixUnparsed
            | Self::EmbossProfileSelectionNative
            | Self::EmbossTargetSelectionNative
            | Self::EmbossDepthUnresolved
            | Self::EmbossDepthSignAssumed
            | Self::EmbossScalarOwnerAssumed
            | Self::EmbossScalarUnitAssumed
            | Self::EmbossPlacementNative
            | Self::AlignOperandsUnresolved
            | Self::AlignEnvelopeUnparsed
            | Self::AlignConstructionUnparsed
            | Self::MoveUnprojected
            | Self::ModifyUnprojected
            | Self::FeatureScopePayloadUninterpreted
            | Self::FeatureFilletSetConflict
            | Self::FeatureChamferSetConflict
            | Self::FeatureScopeEnvelopeSuperseded
            | Self::ParameterUnprojected
            | Self::ParameterOwnerUnparsed
            | Self::ParameterRecordUnparsed
            | Self::ParameterOwnerUnrecognized
            | Self::ParameterTrailerFlagUninterpreted
            | Self::ParameterOwnerUnstored
            | Self::ParameterUnitUntyped
            | Self::MaterialDistanceUnitUntyped
            | Self::FaceAppearanceAmbiguous
            | Self::ParameterExpressionUnbound
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::HistoryDeleteFaceMemberOrderAssumed
            | Self::HistoricalEdgeMidpointsUnresolved
            | Self::HistoricalFilletEdgeInferred
            | Self::HistoricalEdgeSelectionInferred
            | Self::HistoricalEdgeSelectionDeletedSetAssumed
            | Self::HistoricalEdgeHistoryInferred
            | Self::HistoricalOutputBlobHistoryInferred
            | Self::BodyRecipeSerializationFormAssumed
            | Self::HistoricalFilletEdgeAmbiguous
            | Self::HistoricalFilletEdgeCovering
            | Self::HistoricalBodyBoundsUnresolved
            | Self::HistoricalBodyCenterPartial
            | Self::HistoricalSplitPlaneInferred
            | Self::SketchRelationNativeRetained
            | Self::SketchDimensionNativeRetained
            | Self::SketchPlacementUnprojected
            | Self::SketchPointUnprojected
            | Self::SketchPointUndecoded
            | Self::SketchCurveUnprojected
            | Self::SketchSurfaceUnprojected
            | Self::SketchTextUnprojected
            | Self::SketchTextStyleUnprojected
            | Self::SketchRelationUnprojected
            | Self::SketchPatternStateUnprojected
            | Self::SketchGeometryFlagUnprojected
            | Self::SketchProfileSearchCapped
            | Self::SketchCurveUnprofiled
            | Self::DimensionUnprojected
            | Self::FeatureProfileSelectionNative
            | Self::FeatureExtrudeProfileGroupNative
            | Self::FeatureExtrudeSurfaceProfileAssumed
            | Self::FeatureExtrudeSymmetricDistanceAssumed
            | Self::FeatureExtrudeWallThicknessNative
            | Self::FeatureExtrudeAuxiliarySelectionNative
            | Self::FeatureExtrudeFaceControlAssumed
            | Self::FeatureExtrudeSketchPlacementNative
            | Self::FeatureExtrudeTargetOptionAssumed
            | Self::FeatureExtrudeStartFaceAbsentAssumed
            | Self::FeatureExtrudeZeroSideTaperIgnored
            | Self::FeatureExtrudeProfileFaceOrderAssumed
            | Self::FeatureExtrudeProfileFaceSearchCapped
            | Self::FeatureExtrudeProfileCapFacesInferred
            | Self::FeatureExtrudeProfileDeletedFacesInferred
            | Self::FeatureExtrudeProfileFaceSetDisputed
            | Self::FeatureSketchProfileFlagsAssumed
            | Self::FeatureProfileArrangementCapped
            | Self::FeatureProfileDensificationCapped
            | Self::FeatureProfileCrossingLoopCapped
            | Self::FeatureProfileCrossingLoopAssumed
            | Self::FeatureProfileCoincidentDirectionAssumed
            | Self::FeatureProfileLoopFlagsAssumed
            | Self::FeatureProfileRegionIdentitiesStale
            | Self::FeatureProfileCutRegionsAssumed
            | Self::FeatureProfileIslandAbsenceAssumed
            | Self::FeatureProfilePolylineSideAssumed
            | Self::FeatureProfileCapOutlineAssumed
            | Self::FeatureProfileCapCarrierAssumed
            | Self::FeatureProfileHoleBoundaryInferred
            | Self::FeatureProfileRegionHoleInferred
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::DeleteFaceFrameControlsAssumed
            | Self::DeleteFaceGroupSelectionAssumed
            | Self::SurfaceDeleteFaceWidePrologueAssumed
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureSurfaceOffsetGroupOrdinalsAssumed
            | Self::FeatureFormCageOrderAssumed
            | Self::FeatureFormEmptyCagesAssumed
            | Self::FeatureScaleFrameLengthFormAssumed
            | Self::FeatureScaleMatrixWordAssumed
            | Self::FeatureScaleUnlistedGroupsAssumed
            | Self::FeatureWorkAxisRuleUnassigned
            | Self::FeatureWorkAxisEndpointToleranceAssumed
            | Self::FeatureBodySelectionNative
            | Self::FeatureReverseNormalBodyUnverified
            | Self::FeatureFaceGroupsTypeUnverified
            | Self::FeatureFaceGroupsPrefixUnparsed
            | Self::FeatureFaceGroupsSourceAssumed
            | Self::FeatureFaceGroupsSourceTargetUnverified
            | Self::FeatureFaceGroupsRelatedMeshTargetUnverified
            | Self::FeatureFaceGroupsRelatedMeshRoleUnassigned
            | Self::FeatureFaceGroupsGroupHintRoleUnassigned
            | Self::FeatureFaceGroupsSourceUnavailable
            | Self::FeatureFaceGroupsGroupRolesAssumed
            | Self::FeatureFaceGroupsMemberRunUnrepresentable
            | Self::FeatureFaceGroupsMethodAssumed
            | Self::FeatureFaceGroupsMethodUnmapped
            | Self::FeatureFaceGroupsBoundaryDistanceAssumed
            | Self::FeatureFaceGroupsSettingUnassigned
            | Self::UnstitchFrameAssumed
            | Self::UnstitchGroupSelectionAssumed
            | Self::UnstitchPrologueControlsAssumed
            | Self::FeatureCombineSelectionNative
            | Self::FeatureCombineDefinitionUnresolved
            | Self::FeatureCombineMiddleListOmittedAssumed
            | Self::FeatureCombineComponentDestinationAssumed
            | Self::FeatureCombineNamedTailPrologueAssumed
            | Self::FeatureCombinePrologueAssumed
            | Self::FeatureCombineKeptToolsCutAssumed
            | Self::FeatureCombineOperandListRefused
            | Self::FeatureCombineToolConsumptionAssumed
            | Self::FeatureFixedFilletAssignmentNative
            | Self::FeatureExtrudeExtentNative
            | Self::FeatureExtrudeTerminationMissing
            | Self::FeatureRevolveLayoutAssumed
            | Self::FeatureMoveTransformAssumed
            | Self::FeatureMoveTransformReferencesAssumed
            | Self::FeatureMoveScopeReferencesAssumed
            | Self::FeatureMoveFaceScopeReferencesAssumed
            | Self::FeatureMoveFaceSelectionUnresolved
            | Self::FeatureMoveFaceTypeAssumed
            | Self::FeatureMoveFaceTypeConflict
            | Self::FeatureMoveFacePrologueAssumed
            | Self::FeatureMoveFaceMatrixAssumed
            | Self::FeatureMoveTransformModeAssumed
            | Self::FeatureMoveTransformPrefixAssumed
            | Self::FeatureMoveTransformIdentityAssumed
            | Self::PositionEnvelopeUnparsed
            | Self::PositionSemanticsAssumed
            | Self::PositionIdentityAssumed
            | Self::PositionOccurrenceUnresolved
            | Self::PositionTransformUnprojected
            | Self::FeatureAlignMotionAssumed
            | Self::FeatureAlignIdentityAssumed
            | Self::FeatureAlignMotionUnprojected
            | Self::FeatureModifyUnlistedGroupsAssumed
            | Self::FeatureModifyTableAssumed
            | Self::FeatureModifyStateAssumed
            | Self::FeatureLoftReferenceUnresolved
            | Self::FeatureLoftSettingsAssumed
            | Self::FeatureLoftListsUnresolved
            | Self::FeatureLoftGroupsUnassigned
            | Self::FeatureLoftCenterlineAssumed
            | Self::FeatureSurfaceExtendMethodUnresolved
            | Self::FeatureSurfaceExtendDistanceNonpositive
            | Self::FeatureSurfaceBoundaryFlagRecordAssumed
            | Self::FeaturePathPatternFeatureSeedAssumed
            | Self::FeaturePathPatternReorderedSettingsAssumed
            | Self::FeaturePathPatternSeedUnresolved
            | Self::FeaturePathPatternPathUnresolved
            | Self::FeaturePathPatternDefinitionPartial
            | Self::FeaturePathPatternCountOutOfRange
            | Self::FeaturePathPatternDistanceNonpositive
            | Self::FeaturePathPatternStartUnresolved
            | Self::FeaturePathPatternSettingsUnresolved
            | Self::FeaturePathPatternPlacementNotTransferred
            | Self::FeatureRectangularPatternPlacementNotTransferred
            | Self::FeatureThickenFlagsAssumed
            | Self::ThickenTargetSelectionAssumed
            | Self::ThickenThicknessUnresolved
            | Self::ThickenThicknessUnrepresentable
            | Self::ThickenFrameAssumed
            | Self::ThickenFaceSelectionAssumed
            | Self::FeatureShellTableMembersAssumed
            | Self::FeatureShellSourceUnavailable
            | Self::FeatureShellEnvelopeAssumed
            | Self::FeatureShellCountedControlAssumed
            | Self::FeatureShellSlotDirectionAssumed
            | Self::FeatureShellSelectionUnresolved
            | Self::FeatureShellWallValueUnresolved
            | Self::FeatureShellGroupRolesAssumed
            | Self::FeatureShellControlsUninterpreted
            | Self::FeatureShellIndependentWallsNative
            | Self::FeatureShellPrefixFlagAssumed
            | Self::FeatureFaceOperandUnresolved
            | Self::FeatureTrimCellTableNative
            | Self::FeatureTrimMemberTypeAssumed
            | Self::FeatureFaceOperandFallbackContradicted
            | Self::FeatureExtrudeTargetDisputed
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost
            | Self::FeaturePatternInstanceSearchCapped
            | Self::BrepBlobUndecoded
            | Self::MeshContainerUnjoined
            | Self::MeshContainerMissing
            | Self::MeshAttributeNotTransferred
            | Self::MeshSplitStreamAssumed
            | Self::MeshHistoryWindowAssumed
            | Self::MeshOperationRecordsNative
            | Self::MeshFaceGroupsAssumed
            | Self::MeshBodyWordsNative
            | Self::MeshCollectionWordsNative
            | Self::XrefTableUndecoded
            | Self::XrefPlacementUndecoded
            | Self::XrefOccurrenceUnplaced
            | Self::XrefPlacementIdentityAssumed
            | Self::XrefPlacementRoleUnbound
            | Self::XrefCurrentPoseAmbiguous
            | Self::LocalOccurrenceCurrentPoseAmbiguous
            | Self::LocalOccurrencePathPose
            | Self::LocalOccurrencePathPoseUnresolved
            | Self::LocalOccurrencePathPoseDisagrees
            | Self::LocalOccurrencePathPoseInferred
            | Self::XrefContainedPoseDisagrees
            | Self::XrefOwnedPoseDisagrees
            | Self::XrefPlacementMetadataMissing
            | Self::XrefPlacementSuperseded
            | Self::XrefOwnerUnresolved
            | Self::JointPlacedOccurrenceUnsolved
            | Self::MeshVertexPrecisionReduced
            | Self::MeshBodyIdentityAssumed
            | Self::FaceSurfaceReferenceDangling
            | Self::SurfaceShapeNotDecoded
            | Self::ProceduralCurveUndecoded
            | Self::VertexToleranceWidened
            | Self::ProceduralSupportChartUnresolved
            | Self::SpringSupportPcurveUnresolved
            | Self::PcurveUndecoded
            | Self::BlendSupportPartial
            | Self::SolvedRecordUntyped
            | Self::MaterialNotTransferred
            | Self::TsplineRecordUntyped
            | Self::TsplineRadialMapUnresolved
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::KernelDialectUnverified
            | Self::KernelCarrierUnparseable
            | Self::DialectLayerCollision
            | Self::TextHeaderNoncanonical
            | Self::KernelHeaderToleranceUnresolved
            | Self::KernelHeaderMetadataUnresolved
            | Self::ManifestMetadataUnresolved
            | Self::ArchiveEntryUnreadable
            | Self::UnindexedRecordsRetained
            | Self::BodyDisplayNameAmbiguous
            | Self::WorkPlaneDisplayNameAmbiguous
            | Self::WorkPlaneDisplayNameUndecoded => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::SceneOverrideRefused => LossTaxonomy::RecordNotTyped,
            Self::BendStationaryRoleAssumed
            | Self::BendAngleUnresolved
            | Self::BendRadiusFirstAssumed
            | Self::BendLineOwnerOrderAssumed
            | Self::BendStationarySelectionNative
            | Self::BendRuleRadiusUnresolved
            | Self::BendAngleOwnerBacklinkAssumed
            | Self::BendCachedOwnerAngleAssumed
            | Self::UnfoldAllBendsFlagAssumed
            | Self::UnfoldControlUnresolved
            | Self::UnfoldSelectionSlotsAssumed
            | Self::UnfoldSelectionNative
            | Self::UnfoldEnvelopeUnresolved
            | Self::UnfoldMaterialScalarFactorAssumed
            | Self::UnfoldRuleFactorUnresolved
            | Self::UnfoldSettingsRuleReferenceAssumed
            | Self::RefoldGroupSourceAssumed
            | Self::RefoldSourceUnresolved => LossTaxonomy::FeatureHistoryRetained,
            Self::SceneOverrideAmbiguous | Self::SceneOverrideUnbound => {
                LossTaxonomy::FeatureHistoryRetained
            }
            Self::ScopeOutputOwnerMismatch
            | Self::ScopeOutputOwnerUnresolved
            | Self::ScopeSketchOwnerMismatch => LossTaxonomy::FeatureHistoryRetained,
            Self::WriterAppearancePropertiesOmitted => LossTaxonomy::AppearanceReduced,
            Self::DimensionCompanionUntyped
            | Self::BodyReferenceTailUntyped
            | Self::PlacementConstructionUntyped
            | Self::ConstructionTrackingPathUntyped
            | Self::ParameterOwnerUnparsed
            | Self::ParameterRecordUnparsed
            | Self::HistoryRecordFramingFailed
            | Self::SolvedRecordUntyped
            | Self::KernelCarrierUnparseable
            | Self::TsplineRecordUntyped => LossTaxonomy::RecordNotTyped,
            Self::ScopeOwnerUnresolved
            | Self::ConstructionClosingFlagAssumed
            | Self::ChamferEdgeRoleAssumed
            | Self::ChamferDefinitionUnresolved
            | Self::ChamferSelectionUnresolved
            | Self::ChamferReferenceFaceAssumed
            | Self::ChamferAngleReferenceAssumed
            | Self::ChamferCornerTreatmentUnresolved
            | Self::ChamferTangentChainAssumed
            | Self::ChamferParameterOwnerAssumed
            | Self::ChamferParameterValueAssumed
            | Self::ChamferParameterKindAssumed
            | Self::ChamferParameterUnitAssumed
            | Self::CoilTargetMembersAssumed
            | Self::OperationPoseApplied
            | Self::OperationPoseDisagrees
            | Self::OperationPoseNative
            | Self::ConfigurationMemberUnassigned
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::ConfigurationSuppressionInferred
            | Self::ConfigurationActiveRowUnbound
            | Self::DrawingDocumentOmitted
            | Self::XrefPlacementMetadataNative
            | Self::XrefPlacementCurrentPose
            | Self::XrefCurrentPoseNative
            | Self::XrefCurrentPoseAmbiguous
            | Self::LocalOccurrenceCurrentPose
            | Self::LocalOccurrenceCurrentPoseNative
            | Self::LocalOccurrenceCurrentPoseAmbiguous
            | Self::LocalOccurrencePathPose
            | Self::LocalOccurrencePathPoseUnresolved
            | Self::LocalOccurrencePathPoseDisagrees
            | Self::LocalOccurrencePathPoseInferred
            | Self::XrefContainedPose
            | Self::XrefContainedPoseDisagrees
            | Self::XrefContainedPoseNative
            | Self::XrefOwnedPose
            | Self::XrefOwnedPoseDisagrees
            | Self::XrefOwnedPoseNative
            | Self::MeshGuidBytesUninterpreted
            | Self::MeshSplitStreamAssumed
            | Self::MeshHistoryWindowAssumed
            | Self::MeshOperationRecordsNative
            | Self::MeshFaceGroupsAssumed
            | Self::MeshBodyWordsNative
            | Self::MeshCollectionWordsNative
            | Self::MeshBodyBytesUninterpreted
            | Self::MeshWrapperBytesUninterpreted
            | Self::MeshOwnerBytesUninterpreted
            | Self::MeshScopePrefixUninterpreted
            | Self::XrefTableUndecoded => LossTaxonomy::MetadataNotTransferred,
            Self::ActComponentLinkUnresolved
            | Self::AssemblyComponentsExternal
            | Self::XrefCycle
            | Self::XrefMemberMissing
            | Self::XrefMemberUndecoded
            | Self::XrefPlacementUndecoded
            | Self::XrefOccurrenceUnplaced
            | Self::XrefPlacementIdentityAssumed
            | Self::XrefPlacementRoleUnbound
            | Self::XrefPlacementMetadataMissing
            | Self::XrefPlacementSuperseded
            | Self::JointPlacedOccurrenceUnsolved => LossTaxonomy::AssemblyComponentsExternal,
            Self::HistoryBindingBudgetExceeded
            | Self::HistoryBindingWorkExceeded
            | Self::RecipeSideSearchBudgetExceeded
            | Self::ParameterOrderBudgetExceeded
            | Self::FeatureStoredBodiesEmpty
            | Self::FeatureStoredBodiesRecipeAbsent
            | Self::FeatureOutputBodyUnbound
            | Self::FeatureOutputStateChainUnbound
            | Self::FeatureStoredBodiesUnbound
            | Self::FeatureDerivedInstanceComponentInsertAssumed
            | Self::FeatureComponentInsertMembershipAssumed
            | Self::FeatureDerivedInstanceSourcesUnresolved
            | Self::FeatureDerivedInstanceCarrierPlacementAssumed
            | Self::FeatureDerivedInstanceScopePlacementAssumed
            | Self::FeatureDerivedInstanceContextTransformAssumed
            | Self::FeatureDerivedInstancePlacementUnrepresentable
            | Self::FeatureDerivedInstanceDefinitionUnresolved
            | Self::FeaturePasteSourcesUnresolved
            | Self::FeatureOccurrenceTransferSourceUnresolved
            | Self::FeatureOccurrenceTransferOwnerUnresolved
            | Self::FeatureOccurrenceTransferOccurrenceUnresolved
            | Self::FeatureOccurrenceTransferPlacementUnresolved
            | Self::FeatureTimelineOwnerCompeting
            | Self::FeatureTimelineNullSlotAssumed
            | Self::FeatureTimelineOwnerUnpaired
            | Self::FeaturePasteCopyPlacementAssumed
            | Self::FeatureMirrorPlaneWorkPlaneAssumed
            | Self::FeatureCircularPatternPlacementNotTransferred
            | Self::FeatureCircularPatternDistributionAssumed
            | Self::FeatureDerivedBaseUnresolved
            | Self::FeaturePipeCircularSectionInferred
            | Self::FeaturePipeWholePathExtentAssumed
            | Self::FeaturePipeCircularSectionAssumed
            | Self::FeaturePipePathEndSectionAssumed
            | Self::FeatureSweepSourceUnavailable
            | Self::FeatureSweepOperationPrefixAssumed
            | Self::FeatureSweepOperationUnresolved
            | Self::FeatureSweepSectionRoleAssumed
            | Self::FeatureSweepProfileCarrierAssumed
            | Self::FeatureSweepSelectionUnresolved
            | Self::FeatureSweepScalarUnresolved
            | Self::FeatureSweepTargetsNative
            | Self::FeatureSweepResultModeUnresolved
            | Self::FeatureSweepResultModeInferred
            | Self::FeatureSweepPathTangentAssumedFalse
            | Self::FeatureSweepLinearizeAssumedFalse
            | Self::FeatureSweepControlsUninterpreted
            | Self::FeaturePipeSourceUnresolved
            | Self::FeaturePipeOperationUnresolved
            | Self::FeaturePipeHollowUnresolved
            | Self::FeaturePipeScalarUnresolved
            | Self::FeaturePipeSectionUnresolved
            | Self::FeaturePipePathUnresolved
            | Self::FeaturePipePathExtentUnresolved
            | Self::FeaturePipePathParameterizationAssumed
            | Self::FeaturePipePathRoleAssumed
            | Self::FeaturePipeTargetsRetainedNative
            | Self::FeaturePipeTargetsUnresolved
            | Self::FeaturePipePointOriginUnresolved
            | Self::FeaturePipePathTangentAssumedFalse
            | Self::FeaturePipeLinearizeAssumedFalse
            | Self::SurfacePatchInteriorConstraintAssumed
            | Self::SurfacePatchUnnamedGroupsAssumedInactive
            | Self::SurfacePatchUnorderedReferenceRunsAssumed
            | Self::FeatureBoxPrimitiveUnresolved
            | Self::FeatureBoxOffsetFrameAssumed
            | Self::FeatureBoxSupportFrameAssumed
            | Self::FeatureBoxSupportFlagsAssumed
            | Self::FeatureDefinitionIncomplete
            | Self::FeatureMeshImportTessellationsUnresolved
            | Self::FeatureMeshImportEmptySourceAssumed
            | Self::FeatureMeshImportSourceUnparsed
            | Self::FeatureMeshSectionPlaneUnresolved
            | Self::FeatureMeshShellControlsUnresolved
            | Self::FeatureMeshOperationTailPartial
            | Self::FeatureMeshBodySelectionAssumed
            | Self::FeatureMeshOperationSourceUnresolved
            | Self::FeatureMeshOperationScopeUnparsed
            | Self::FeatureMeshOperationSelectorUnruled
            | Self::FeatureMeshOperationParameterUnbound
            | Self::FeatureMeshOperationFieldsUninterpreted
            | Self::FeatureMeshSectionCurvesUnresolved
            | Self::FeatureMeshConversionScopeUnparsed
            | Self::FeatureMeshConversionTailPartial
            | Self::FeatureMeshConversionMethodUnresolved
            | Self::FeatureMeshConversionMethodAssumed
            | Self::FeatureMeshConversionEnvelopeAssumed
            | Self::FeatureMeshConversionSourceUnresolved
            | Self::FeatureMeshConversionOutputUnresolved
            | Self::ThreadLiveFaceUnproven
            | Self::SheetThicknessUnruled
            | Self::WorkPlaneFrameUnbound
            | Self::WorkPlaneInferredAxisAssumed
            | Self::CylinderPlacementUnbound
            | Self::SpherePrefixFramingAssumed
            | Self::SpherePlacementFlagsAssumed
            | Self::SphereLocalMatrixFramingAssumed
            | Self::SphereLocalFrameAssumed
            | Self::SpherePlacementFrameAssumed
            | Self::SphereDiameterOwnerAssumed
            | Self::SphereDiameterSourceAssumed
            | Self::HoleExtentUnproven
            | Self::HolePlacementUnproven
            | Self::HoleReferenceDimensionsNative
            | Self::HoleParametersIndependentAssumed
            | Self::HoleShapePartialAssumed
            | Self::HoleExtentLeadingWordAssumed
            | Self::HoleTappedDiameterStored
            | Self::HoleStandardSizingAssumed
            | Self::HoleCarrierAxisAssumed
            | Self::FeatureScopeUnprojected
            | Self::EmbossControlsNative
            | Self::FeatureShellAuxiliaryReferencesNative
            | Self::EmbossPrefixUnparsed
            | Self::EmbossProfileSelectionNative
            | Self::EmbossTargetSelectionNative
            | Self::EmbossDepthUnresolved
            | Self::EmbossDepthSignAssumed
            | Self::EmbossScalarOwnerAssumed
            | Self::EmbossScalarUnitAssumed
            | Self::EmbossPlacementNative
            | Self::AlignOperandsUnresolved
            | Self::AlignEnvelopeUnparsed
            | Self::AlignConstructionUnparsed
            | Self::MoveUnprojected
            | Self::ModifyUnprojected
            | Self::FeatureScopePayloadUninterpreted
            | Self::FeatureFilletSetConflict
            | Self::FeatureChamferSetConflict
            | Self::FeatureScopeEnvelopeSuperseded
            | Self::ParameterUnprojected
            | Self::ParameterOwnerUnrecognized
            | Self::ParameterTrailerFlagUninterpreted
            | Self::ParameterOwnerUnstored
            | Self::ParameterUnitUntyped
            | Self::ParameterExpressionUnbound
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::HistoryDeleteFaceMemberOrderAssumed
            | Self::HistoricalEdgeMidpointsUnresolved
            | Self::HistoricalFilletEdgeInferred
            | Self::HistoricalEdgeSelectionInferred
            | Self::HistoricalEdgeSelectionDeletedSetAssumed
            | Self::HistoricalEdgeHistoryInferred
            | Self::HistoricalOutputBlobHistoryInferred
            | Self::BodyRecipeSerializationFormAssumed
            | Self::HistoricalFilletEdgeAmbiguous
            | Self::HistoricalFilletEdgeCovering
            | Self::HistoricalBodyBoundsUnresolved
            | Self::HistoricalBodyCenterPartial
            | Self::HistoricalSplitPlaneInferred
            | Self::SketchRelationNativeRetained
            | Self::SketchDimensionNativeRetained
            | Self::SketchPlacementUnprojected
            | Self::SketchPointUnprojected
            | Self::SketchCurveUnprojected
            | Self::SketchSurfaceUnprojected
            | Self::SketchTextUnprojected
            | Self::SketchTextStyleUnprojected
            | Self::SketchRelationUnprojected
            | Self::SketchPatternStateUnprojected
            | Self::SketchGeometryFlagUnprojected
            | Self::SketchProfileSearchCapped
            | Self::SketchCurveUnprofiled
            | Self::DimensionUnprojected
            | Self::FeatureProfileSelectionNative
            | Self::FeatureExtrudeProfileGroupNative
            | Self::FeatureExtrudeSurfaceProfileAssumed
            | Self::FeatureExtrudeSymmetricDistanceAssumed
            | Self::FeatureExtrudeWallThicknessNative
            | Self::FeatureExtrudeAuxiliarySelectionNative
            | Self::FeatureExtrudeFaceControlAssumed
            | Self::FeatureExtrudeSketchPlacementNative
            | Self::FeatureExtrudeTargetOptionAssumed
            | Self::FeatureExtrudeStartFaceAbsentAssumed
            | Self::FeatureExtrudeZeroSideTaperIgnored
            | Self::FeatureExtrudeProfileFaceOrderAssumed
            | Self::FeatureExtrudeProfileFaceSearchCapped
            | Self::FeatureExtrudeProfileCapFacesInferred
            | Self::FeatureExtrudeProfileDeletedFacesInferred
            | Self::FeatureExtrudeProfileFaceSetDisputed
            | Self::FeatureSketchProfileFlagsAssumed
            | Self::FeatureProfileArrangementCapped
            | Self::FeatureProfileDensificationCapped
            | Self::FeatureProfileCrossingLoopCapped
            | Self::FeatureProfileCrossingLoopAssumed
            | Self::FeatureProfileCoincidentDirectionAssumed
            | Self::FeatureProfileLoopFlagsAssumed
            | Self::FeatureProfileRegionIdentitiesStale
            | Self::FeatureProfileCutRegionsAssumed
            | Self::FeatureProfileIslandAbsenceAssumed
            | Self::FeatureProfilePolylineSideAssumed
            | Self::FeatureProfileCapOutlineAssumed
            | Self::FeatureProfileCapCarrierAssumed
            | Self::FeatureProfileHoleBoundaryInferred
            | Self::FeatureProfileRegionHoleInferred
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::DeleteFaceFrameControlsAssumed
            | Self::DeleteFaceGroupSelectionAssumed
            | Self::SurfaceDeleteFaceWidePrologueAssumed
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureSurfaceOffsetGroupOrdinalsAssumed
            | Self::FeatureFormCageOrderAssumed
            | Self::FeatureFormEmptyCagesAssumed
            | Self::FeatureScaleFrameLengthFormAssumed
            | Self::FeatureScaleMatrixWordAssumed
            | Self::FeatureScaleUnlistedGroupsAssumed
            | Self::FeatureWorkAxisRuleUnassigned
            | Self::FeatureWorkAxisEndpointToleranceAssumed
            | Self::FeatureBodySelectionNative
            | Self::FeatureReverseNormalBodyUnverified
            | Self::FeatureFaceGroupsTypeUnverified
            | Self::FeatureFaceGroupsPrefixUnparsed
            | Self::FeatureFaceGroupsSourceAssumed
            | Self::FeatureFaceGroupsSourceTargetUnverified
            | Self::FeatureFaceGroupsRelatedMeshTargetUnverified
            | Self::FeatureFaceGroupsRelatedMeshRoleUnassigned
            | Self::FeatureFaceGroupsGroupHintRoleUnassigned
            | Self::FeatureFaceGroupsSourceUnavailable
            | Self::FeatureFaceGroupsGroupRolesAssumed
            | Self::FeatureFaceGroupsMemberRunUnrepresentable
            | Self::FeatureFaceGroupsMethodAssumed
            | Self::FeatureFaceGroupsMethodUnmapped
            | Self::FeatureFaceGroupsBoundaryDistanceAssumed
            | Self::FeatureFaceGroupsSettingUnassigned
            | Self::UnstitchFrameAssumed
            | Self::UnstitchGroupSelectionAssumed
            | Self::UnstitchPrologueControlsAssumed
            | Self::FeatureCombineSelectionNative
            | Self::FeatureCombineDefinitionUnresolved
            | Self::FeatureCombineMiddleListOmittedAssumed
            | Self::FeatureCombineComponentDestinationAssumed
            | Self::FeatureCombineNamedTailPrologueAssumed
            | Self::FeatureCombinePrologueAssumed
            | Self::FeatureCombineKeptToolsCutAssumed
            | Self::FeatureCombineOperandListRefused
            | Self::FeatureCombineToolConsumptionAssumed
            | Self::FeatureFixedFilletAssignmentNative
            | Self::FeatureExtrudeExtentNative
            | Self::FeatureExtrudeTerminationMissing
            | Self::FeatureRevolveLayoutAssumed
            | Self::FeatureMoveTransformAssumed
            | Self::FeatureMoveTransformReferencesAssumed
            | Self::FeatureMoveScopeReferencesAssumed
            | Self::FeatureMoveFaceScopeReferencesAssumed
            | Self::FeatureMoveFaceSelectionUnresolved
            | Self::FeatureMoveFaceTypeAssumed
            | Self::FeatureMoveFaceTypeConflict
            | Self::FeatureMoveFacePrologueAssumed
            | Self::FeatureMoveFaceMatrixAssumed
            | Self::FeatureMoveTransformModeAssumed
            | Self::FeatureMoveTransformPrefixAssumed
            | Self::FeatureMoveTransformIdentityAssumed
            | Self::PositionEnvelopeUnparsed
            | Self::PositionSemanticsAssumed
            | Self::PositionIdentityAssumed
            | Self::PositionOccurrenceUnresolved
            | Self::PositionTransformUnprojected
            | Self::FeatureAlignMotionAssumed
            | Self::FeatureAlignIdentityAssumed
            | Self::FeatureAlignMotionUnprojected
            | Self::FeatureModifyUnlistedGroupsAssumed
            | Self::FeatureModifyTableAssumed
            | Self::FeatureModifyStateAssumed
            | Self::FeatureLoftReferenceUnresolved
            | Self::FeatureLoftSettingsAssumed
            | Self::FeatureLoftListsUnresolved
            | Self::FeatureLoftGroupsUnassigned
            | Self::FeatureLoftCenterlineAssumed
            | Self::FeatureSurfaceExtendMethodUnresolved
            | Self::FeatureSurfaceExtendDistanceNonpositive
            | Self::FeatureSurfaceBoundaryFlagRecordAssumed
            | Self::FeaturePathPatternFeatureSeedAssumed
            | Self::FeaturePathPatternReorderedSettingsAssumed
            | Self::FeaturePathPatternSeedUnresolved
            | Self::FeaturePathPatternPathUnresolved
            | Self::FeaturePathPatternDefinitionPartial
            | Self::FeaturePathPatternCountOutOfRange
            | Self::FeaturePathPatternDistanceNonpositive
            | Self::FeaturePathPatternStartUnresolved
            | Self::FeaturePathPatternSettingsUnresolved
            | Self::FeaturePathPatternPlacementNotTransferred
            | Self::FeatureRectangularPatternPlacementNotTransferred
            | Self::FeatureThickenFlagsAssumed
            | Self::ThickenTargetSelectionAssumed
            | Self::ThickenThicknessUnresolved
            | Self::ThickenThicknessUnrepresentable
            | Self::ThickenFrameAssumed
            | Self::ThickenFaceSelectionAssumed
            | Self::FeatureShellTableMembersAssumed
            | Self::FeatureShellSourceUnavailable
            | Self::FeatureShellEnvelopeAssumed
            | Self::FeatureShellCountedControlAssumed
            | Self::FeatureShellSlotDirectionAssumed
            | Self::FeatureShellSelectionUnresolved
            | Self::FeatureShellWallValueUnresolved
            | Self::FeatureShellGroupRolesAssumed
            | Self::FeatureShellControlsUninterpreted
            | Self::FeatureShellIndependentWallsNative
            | Self::FeatureShellPrefixFlagAssumed
            | Self::FeatureFaceOperandUnresolved
            | Self::FeatureTrimCellTableNative
            | Self::FeatureTrimMemberTypeAssumed
            | Self::FeatureFaceOperandFallbackContradicted
            | Self::FeatureExtrudeTargetDisputed
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost
            | Self::FeatureOccurrencePathUnresolved
            | Self::FeaturePatternInstanceSearchCapped
            | Self::AssemblyAlignmentPartial
            | Self::AssemblyJointUnprojected
            | Self::AssemblyRigidGroupFrameUnparsed
            | Self::AssemblyJointMotionUndecoded
            | Self::AssemblyJointAngularLimitsNative
            | Self::AssemblyJointRestUnprojected
            | Self::AssemblyJointActivationAssumed
            | Self::AssemblyOperandPathPairsNative
            | Self::AssemblyOperandDescentUnresolved
            | Self::AssemblyJointSourceBindingAssumed
            | Self::AssemblyConnectorOperandsAssumed
            | Self::AssemblyMotionBindingAssumed
            | Self::AssemblyAdditionalOwnerMotionAssumed
            | Self::AssemblyExtendedTailAssumed
            | Self::AssemblyExtendedTypedTailAssumed
            | Self::AssemblyOperandNestedPathRetained
            | Self::AssemblyOperandDescentAmbiguous
            | Self::AssemblyRootPathIdentitiesNative
            | Self::HistoryOnlyBodyOmitted => LossTaxonomy::FeatureHistoryRetained,
            Self::MaterialDistanceUnitUntyped => LossTaxonomy::MaterialNotTransferred,
            Self::FaceAppearanceAmbiguous => LossTaxonomy::AppearanceReduced,
            Self::XrefOwnerUnresolved
            | Self::ComponentBodyOwnerUnresolved
            | Self::ComponentBodyOwnerFallback
            | Self::LocalOccurrenceOwnerUnbound
            | Self::LocalOccurrenceOwnerCycle
            | Self::LocalOccurrenceExpansionLimit
            | Self::LocalOccurrenceUnplaced
            | Self::ComponentInsertUnplaced
            | Self::ComponentInsertOrderUnknown
            | Self::ComponentInsertContainerAssumed
            | Self::SavedFreeEdgeBodyUnowned
            | Self::LocalOccurrenceIdentityOrdinalUnresolved
            | Self::LocalOccurrenceSourceUnregistered
            | Self::LocalOccurrenceTypeUnregistered
            | Self::LocalOccurrenceKeyRepeated
            | Self::LocalOccurrenceFirstMatrixRefused
            | Self::LocalOccurrenceEnvelopeNative
            | Self::LocalComponentPathNative
            | Self::LocalOccurrenceEnvelopeSuperseded
            | Self::RootInstanceDefinitionUnbound
            | Self::ComponentDefinitionRevised
            | Self::ComponentNamingSpaceUnbound => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::SketchOwnerUnresolved
            | Self::SketchMembershipConflict
            | Self::SketchFolderClosingReference
            | Self::SketchFolderUnprojected
            | Self::SketchGeometryGroupMemberUnlisted
            | Self::SketchGeometryGroupMemberUnbacklinked
            | Self::DesignBodyBindingUnresolved
            | Self::FaceSurfaceReferenceDangling
            | Self::PcurveUndecoded => LossTaxonomy::ReferenceGraphNotClosed,
            Self::SketchNameComposedFromFeatureOrdinal
            | Self::ReferenceImageNativeRetained
            | Self::ReferenceImageFlagAssumed
            | Self::DecalNativeRetained
            | Self::DecalMappingAssumed
            | Self::DecalTargetUnresolved
            | Self::DecalImageUnresolved
            | Self::DecalDefinitionUnresolved
            | Self::EdgeReferenceLostUnrepaired
            | Self::MeshAttributeNotTransferred
            | Self::TsplineRadialMapUnresolved
            | Self::BodyDisplayNameAmbiguous
            | Self::WorkPlaneDisplayNameAmbiguous
            | Self::WorkPlaneDisplayNameUndecoded => LossTaxonomy::AttributesNotTransferred,
            Self::BrepBlobUndecoded
            | Self::SurfaceShapeNotDecoded
            | Self::GeometryNotTransferred
            | Self::TsplineCageUndecoded
            | Self::SketchPointUndecoded => LossTaxonomy::GeometryNotTransferred,
            Self::MeshContainerUnjoined | Self::MeshContainerMissing => {
                LossTaxonomy::AssetNotTransferred
            }
            Self::MeshContainerUndecoded
            | Self::MeshBodyIdentityAssumed
            | Self::DialectLayerCollision
            | Self::LocalOccurrenceYieldedToRegistered
            | Self::DesignIntentUndecoded => LossTaxonomy::DecodeDiagnostic,
            Self::MeshVertexPrecisionReduced => LossTaxonomy::MeshVertexPrecision,
            Self::BodylessDesignCarrier
            | Self::NurbsSurfaceCarrier
            | Self::NurbsCurveCarrier
            | Self::MeshSurfaceSentinel => LossTaxonomy::CarrierSummary,
            Self::VertexToleranceWidened
            | Self::SketchRegionBodyOmitted
            | Self::SketchCurveRegionBoundary
            | Self::SketchCurveUnprofiledSelected => LossTaxonomy::DecodeDiagnostic,
            Self::ProceduralCurveUndecoded
            | Self::ProceduralSupportChartUnresolved
            | Self::SpringSupportPcurveUnresolved
            | Self::BlendSupportPartial => LossTaxonomy::ProceduralReduced,
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
            Self::ArchiveEntryUnreadable | Self::UnindexedRecordsRetained => {
                LossTaxonomy::RecordNotTyped
            }
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

#[cfg(test)]
mod tests {
    use super::F3dLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = F3dLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "scene.override-refused",
                "scene.override-ambiguous",
                "scene.override-unbound",
                "assembly.operation-table-pose",
                "scope.owner-unresolved",
                "scope.output-owner-mismatch",
                "scope.output-owner-unresolved",
                "scope.sketch-owner-mismatch",
                "assembly.operation-table-pose-disagrees",
                "assembly.operation-table-pose-native",
                "dimension.companion-untyped",
                "body-reference.tail-untyped",
                "body-recipe.serialization-form-assumed",
                "placement.construction-untyped",
                "work-plane.frame-unbound",
                "work-plane.inferred-axis-assumed",
                "feature.cylinder-placement-unbound",
                "feature.sphere-prefix-framing-assumed",
                "feature.sphere-placement-flags-assumed",
                "feature.sphere-local-matrix-framing-assumed",
                "feature.sphere-local-frame-assumed",
                "feature.sphere-placement-frame-assumed",
                "feature.sphere-diameter-owner-assumed",
                "feature.sphere-diameter-source-assumed",
                "feature.tracking-path-untyped",
                "feature.construction-closing-flag-assumed",
                "feature.chamfer-edge-role-assumed",
                "feature.chamfer-definition-unresolved",
                "feature.chamfer-selection-unresolved",
                "feature.chamfer-reference-face-assumed",
                "feature.chamfer-angle-reference-assumed",
                "feature.chamfer-corner-treatment-unresolved",
                "feature.chamfer-tangent-chain-assumed",
                "feature.chamfer-parameter-owner-assumed",
                "feature.chamfer-parameter-value-assumed",
                "feature.chamfer-parameter-kind-assumed",
                "feature.chamfer-parameter-unit-assumed",
                "feature.coil-target-members-assumed",
                "configuration.member-unassigned",
                "configuration.rule-unbound",
                "configuration.parameter-override-unbound",
                "configuration.feature-suppression-unbound",
                "configuration.suppression-inferred",
                "configuration.active-row-unbound",
                "assembly.act-component-link-unresolved",
                "assembly.local-occurrence-unplaced",
                "assembly.local-occurrence-owner-unbound",
                "assembly.component-body-owner-unresolved",
                "assembly.component-body-owner-fallback",
                "assembly.local-occurrence-owner-cycle",
                "assembly.local-occurrence-expansion-limit",
                "feature.occurrence-path-unresolved",
                "sketch.component-owner-unresolved",
                "sketch.name-composed-from-feature-ordinal",
                "sketch.membership-conflict",
                "sketch.folder-closing-reference",
                "sketch.folder-unprojected",
                "sketch.geometry-group-member-unlisted",
                "sketch.geometry-group-member-unbacklinked",
                "assembly.component-insert-unplaced",
                "assembly.component-insert-order-unknown",
                "assembly.component-insert-container-assumed",
                "assembly.component-definition-revised",
                "assembly.component-naming-space-unbound",
                "assembly.root-instance-definition-unbound",
                "assembly.local-occurrence-envelope-superseded",
                "assembly.local-occurrence-envelope-native",
                "assembly.local-component-path-native",
                "assembly.local-occurrence-type-unregistered",
                "assembly.local-occurrence-yielded-to-registered",
                "assembly.local-occurrence-key-repeated",
                "assembly.local-occurrence-first-matrix-refused",
                "assembly.local-occurrence-identity-ordinal-unresolved",
                "assembly.local-occurrence-source-unregistered",
                "assembly.saved-free-edge-body-unowned",
                "assembly.joint-unprojected",
                "assembly.alignment-partial",
                "assembly.rigid-group-frame-unparsed",
                "assembly.joint-motion-undecoded",
                "assembly.joint-rest-unprojected",
                "assembly.joint-activation-assumed",
                "assembly.joint-angular-limits-native",
                "assembly.operand-path-pairs-native",
                "assembly.root-path-identities-native",
                "assembly.operand-descent-ambiguous",
                "assembly.operand-descent-unresolved",
                "assembly.joint-source-binding-assumed",
                "assembly.connector-operands-assumed",
                "assembly.motion-binding-assumed",
                "assembly.additional-owner-motion-assumed",
                "assembly.extended-tail-assumed",
                "assembly.extended-typed-tail-assumed",
                "assembly.operand-nested-path-retained",
                "drawing.document-omitted",
                "history.binding-budget-exceeded",
                "history.binding-work-exceeded",
                "feature.recipe-search-budget-exceeded",
                "parameter.order-budget-exceeded",
                "history.record-framing-failed",
                "design.body-binding-unresolved",
                "design.history-only-body-omitted",
                "appearance.reference-image-native",
                "appearance.reference-image-flag-assumed",
                "appearance.decal-native",
                "appearance.decal-mapping-assumed",
                "appearance.decal-target-unresolved",
                "appearance.decal-image-unresolved",
                "appearance.decal-definition-unresolved",
                "feature.edge-reference-lost",
                "feature.definition-incomplete",
                "feature.mesh-import-tessellations-unresolved",
                "feature.mesh-import-empty-source-assumed",
                "feature.mesh-import-source-unparsed",
                "feature.mesh-section-plane-unresolved",
                "feature.mesh-shell-controls-unresolved",
                "feature.mesh-operation-tail-partial",
                "feature.mesh-body-selection-assumed",
                "feature.mesh-operation-source-unresolved",
                "feature.mesh-operation-scope-unparsed",
                "feature.mesh-operation-selector-unruled",
                "feature.mesh-operation-parameter-unbound",
                "feature.mesh-operation-fields-uninterpreted",
                "feature.mesh-section-curves-unresolved",
                "feature.mesh-conversion-scope-unparsed",
                "feature.mesh-conversion-tail-partial",
                "feature.mesh-conversion-method-unresolved",
                "feature.mesh-conversion-method-assumed",
                "feature.mesh-conversion-envelope-assumed",
                "feature.mesh-conversion-source-unresolved",
                "feature.mesh-conversion-output-unresolved",
                "feature.thread-live-face-unproven",
                "feature.stored-bodies-empty",
                "feature.stored-bodies-recipe-absent",
                "feature.stored-bodies-unbound",
                "feature.derived-instance-component-insert-assumed",
                "feature.component-insert-membership-assumed",
                "feature.derived-instance-sources-unresolved",
                "feature.derived-instance-carrier-placement-assumed",
                "feature.derived-instance-scope-placement-assumed",
                "feature.derived-instance-context-transform-assumed",
                "feature.derived-instance-placement-unrepresentable",
                "feature.derived-instance-definition-unresolved",
                "feature.paste-sources-unresolved",
                "feature.occurrence-transfer-source-unresolved",
                "feature.occurrence-transfer-owner-unresolved",
                "feature.occurrence-transfer-occurrence-unresolved",
                "feature.occurrence-transfer-placement-unresolved",
                "feature.timeline-owner-competing",
                "feature.timeline-null-slot-assumed",
                "feature.timeline-owner-unpaired",
                "feature.paste-copy-placement-assumed",
                "feature.mirror-plane-work-plane-assumed",
                "feature.circular-pattern-placement-not-transferred",
                "feature.circular-pattern-distribution-assumed",
                "feature.output-state-chain-unbound",
                "feature.output-body-unbound",
                "feature.derived-base-unresolved",
                "feature.pipe-circular-section-inferred",
                "feature.pipe-whole-path-extent-assumed",
                "feature.pipe-circular-section-assumed",
                "feature.pipe-path-end-section-assumed",
                "feature.sweep-source-unavailable",
                "feature.sweep-operation-prefix-assumed",
                "feature.sweep-operation-unresolved",
                "feature.sweep-section-role-assumed",
                "feature.sweep-profile-carrier-assumed",
                "feature.sweep-selection-unresolved",
                "feature.sweep-scalar-unresolved",
                "feature.sweep-targets-native",
                "feature.sweep-result-mode-unresolved",
                "feature.sweep-result-mode-inferred",
                "feature.sweep-path-tangent-assumed-false",
                "feature.sweep-linearize-assumed-false",
                "feature.sweep-controls-uninterpreted",
                "feature.pipe-source-unresolved",
                "feature.pipe-operation-unresolved",
                "feature.pipe-hollow-unresolved",
                "feature.pipe-scalar-unresolved",
                "feature.pipe-section-unresolved",
                "feature.pipe-path-unresolved",
                "feature.pipe-path-extent-unresolved",
                "feature.pipe-path-parameterization-assumed",
                "feature.pipe-path-role-assumed",
                "feature.pipe-targets-retained-native",
                "feature.pipe-targets-unresolved",
                "feature.pipe-point-origin-unresolved",
                "feature.pipe-path-tangent-assumed-false",
                "feature.pipe-linearize-assumed-false",
                "feature.surface-patch-interior-constraint-assumed",
                "feature.surface-patch-unnamed-groups-assumed-inactive",
                "feature.surface-patch-unordered-reference-runs-assumed",
                "feature.box-primitive-unresolved",
                "feature.box-offset-frame-assumed",
                "feature.box-support-frame-assumed",
                "feature.box-support-flags-assumed",
                "feature.sheet-thickness-unruled",
                "feature.scope-unprojected",
                "feature.move-unprojected",
                "feature.modify-unprojected",
                "feature.hole-extent-unproven",
                "feature.hole-placement-unproven",
                "feature.hole-reference-dimensions-native",
                "feature.hole-parameters-independent-assumed",
                "feature.hole-shape-partial-assumed",
                "feature.hole-extent-leading-word-assumed",
                "feature.hole-tapped-diameter-stored",
                "feature.hole-standard-sizing-assumed",
                "feature.hole-carrier-axis-assumed",
                "feature.align-operands-unresolved",
                "feature.align-envelope-unparsed",
                "feature.align-construction-unparsed",
                "feature.scope-payload-uninterpreted",
                "feature.fillet-set-conflict",
                "feature.chamfer-set-conflict",
                "feature.emboss-controls-native",
                "feature.shell-auxiliary-references-native",
                "feature.emboss-prefix-unparsed",
                "feature.emboss-profile-selection-native",
                "feature.emboss-target-selection-native",
                "feature.emboss-depth-unresolved",
                "feature.emboss-depth-sign-assumed",
                "feature.emboss-scalar-owner-assumed",
                "feature.emboss-scalar-unit-assumed",
                "feature.emboss-placement-native",
                "feature.scope-envelope-superseded",
                "parameter.unprojected",
                "parameter.record-unparsed",
                "parameter.trailer-flag-uninterpreted",
                "parameter.owner-unstored",
                "parameter.owner-unrecognized",
                "parameter.owner-unparsed",
                "parameter.unit-untyped",
                "material.distance-unit-untyped",
                "material.face-appearance-ambiguous",
                "parameter.expression-unbound",
                "history.dependency-unprojected",
                "history.dependency-ambiguous",
                "history.face-selection-unbound",
                "history.delete-face-member-order-assumed",
                "history.edge-midpoints-unresolved",
                "history.fillet-edge-inferred",
                "history.edge-selection-inferred",
                "history.edge-selection-deleted-set-assumed",
                "history.edge-history-inferred",
                "history.output-blob-history-inferred",
                "history.fillet-edge-ambiguous",
                "history.fillet-edge-covering",
                "history.body-bounds-unresolved",
                "history.body-center-partial",
                "history.split-plane-inferred",
                "sketch.relation-native",
                "sketch.dimension-native",
                "sketch.placement-unprojected",
                "sketch.point-unprojected",
                "sketch.point-undecoded",
                "sketch.curve-unprojected",
                "sketch.surface-unprojected",
                "sketch.text-unprojected",
                "sketch.text-style-unprojected",
                "sketch.relation-unprojected",
                "sketch.pattern-state-unprojected",
                "sketch.geometry-flag-unprojected",
                "sketch.profile-search-capped",
                "sketch.curve-unprofiled",
                "sketch.curve-unprofiled-selected",
                "sketch.curve-region-boundary",
                "dimension.unprojected",
                "feature.profile-selection-native",
                "feature.extrude-profile-group-native",
                "feature.extrude-surface-profile-assumed",
                "feature.extrude-symmetric-distance-assumed",
                "feature.extrude-wall-thickness-native",
                "feature.extrude-auxiliary-selection-native",
                "feature.extrude-face-control-assumed",
                "feature.extrude-sketch-placement-native",
                "feature.extrude-target-option-assumed",
                "feature.extrude-start-face-absent-assumed",
                "feature.extrude-zero-side-taper-ignored",
                "feature.extrude-profile-face-order-assumed",
                "feature.extrude-profile-face-search-capped",
                "feature.extrude-profile-cap-faces-inferred",
                "feature.extrude-profile-deleted-faces-inferred",
                "feature.extrude-profile-face-set-disputed",
                "feature.sketch-profile-flags-assumed",
                "feature.profile-arrangement-capped",
                "feature.profile-densification-capped",
                "feature.profile-crossing-loop-capped",
                "feature.profile-crossing-loop-assumed",
                "feature.profile-coincident-direction-assumed",
                "feature.profile-loop-flags-assumed",
                "feature.profile-cut-regions-assumed",
                "feature.profile-region-identities-stale",
                "feature.profile-island-absence-assumed",
                "feature.profile-polyline-side-assumed",
                "feature.profile-cap-outline-assumed",
                "feature.profile-cap-carrier-assumed",
                "feature.profile-hole-boundary-inferred",
                "feature.profile-region-hole-inferred",
                "feature.path-selection-native",
                "feature.face-selection-native",
                "feature.delete-face-frame-controls-assumed",
                "feature.delete-face-group-selection-assumed",
                "feature.surface-delete-face-wide-prologue-assumed",
                "feature.face-selection-active-substituted",
                "feature.surface-offset-group-ordinals-assumed",
                "feature.form-cage-order-assumed",
                "feature.form-empty-cages-assumed",
                "feature.scale-frame-length-form-assumed",
                "feature.scale-matrix-word-assumed",
                "feature.scale-unlisted-groups-assumed",
                "feature.work-axis-rule-unassigned",
                "feature.work-axis-endpoint-tolerance-assumed",
                "feature.body-selection-native",
                "feature.reverse-normal-body-unverified",
                "feature.mesh-face-groups-type-unverified",
                "feature.mesh-face-groups-prefix-unparsed",
                "feature.mesh-face-groups-source-assumed",
                "feature.mesh-face-groups-source-unavailable",
                "feature.mesh-face-groups-source-target-unverified",
                "feature.mesh-face-groups-related-target-unverified",
                "feature.mesh-face-groups-related-role-unassigned",
                "feature.mesh-face-groups-group-hint-role-unassigned",
                "feature.mesh-face-groups-group-roles-assumed",
                "feature.mesh-face-groups-member-run-unrepresentable",
                "feature.mesh-face-groups-method-assumed",
                "feature.mesh-face-groups-method-unmapped",
                "feature.mesh-face-groups-boundary-distance-assumed",
                "feature.mesh-face-groups-setting-unassigned",
                "feature.unstitch-frame-assumed",
                "feature.unstitch-group-selection-assumed",
                "feature.unstitch-prologue-controls-assumed",
                "feature.combine-selection-native",
                "feature.combine-definition-unresolved",
                "feature.combine-middle-list-omitted-assumed",
                "feature.combine-component-destination-assumed",
                "feature.combine-named-tail-prologue-assumed",
                "feature.combine-prologue-assumed",
                "feature.combine-kept-tools-cut-assumed",
                "feature.combine-operand-list-refused",
                "feature.combine-tool-consumption-assumed",
                "feature.fixed-fillet-assignment-native",
                "feature.extrude-extent-native",
                "feature.extrude-termination-missing",
                "feature.revolve-layout-assumed",
                "feature.move-transform-assumed",
                "feature.move-transform-references-assumed",
                "feature.move-scope-references-assumed",
                "feature.move-face-scope-references-assumed",
                "feature.move-face-selection-unresolved",
                "feature.move-face-type-assumed",
                "feature.move-face-type-conflict",
                "feature.move-face-prologue-assumed",
                "feature.move-face-matrix-assumed",
                "feature.move-transform-mode-assumed",
                "feature.move-transform-prefix-assumed",
                "feature.move-transform-identity-assumed",
                "feature.position-envelope-unparsed",
                "feature.position-semantics-assumed",
                "feature.position-identity-assumed",
                "feature.position-occurrence-unresolved",
                "feature.position-transform-unprojected",
                "feature.align-motion-assumed",
                "feature.align-identity-assumed",
                "feature.align-motion-unprojected",
                "feature.modify-unlisted-groups-assumed",
                "feature.modify-table-assumed",
                "feature.modify-state-assumed",
                "feature.loft-reference-unresolved",
                "feature.loft-settings-assumed",
                "feature.loft-lists-unresolved",
                "feature.loft-groups-unassigned",
                "feature.loft-centerline-assumed",
                "feature.surface-extend-method-unresolved",
                "feature.surface-extend-distance-nonpositive",
                "feature.surface-boundary-flag-record-assumed",
                "feature.path-pattern-feature-seed-assumed",
                "feature.path-pattern-reordered-settings-assumed",
                "feature.path-pattern-seed-unresolved",
                "feature.path-pattern-path-unresolved",
                "feature.path-pattern-definition-partial",
                "feature.path-pattern-count-out-of-range",
                "feature.path-pattern-distance-nonpositive",
                "feature.path-pattern-start-unresolved",
                "feature.path-pattern-settings-unresolved",
                "feature.path-pattern-placement-not-transferred",
                "feature.rectangular-pattern-placement-not-transferred",
                "feature.thicken-flags-assumed",
                "feature.thicken-target-selection-assumed",
                "feature.thicken-frame-assumed",
                "feature.thicken-thickness-unresolved",
                "feature.thicken-thickness-unrepresentable",
                "feature.thicken-face-selection-assumed",
                "feature.shell-table-members-assumed",
                "feature.shell-source-unavailable",
                "feature.shell-envelope-assumed",
                "feature.shell-counted-control-assumed",
                "feature.shell-slot-direction-assumed",
                "feature.shell-selection-unresolved",
                "feature.shell-wall-value-unresolved",
                "feature.shell-group-roles-assumed",
                "feature.shell-controls-uninterpreted",
                "feature.shell-independent-walls-native",
                "feature.shell-prefix-flag-assumed",
                "feature.face-operand-unresolved",
                "feature.trim-cell-table-native",
                "feature.trim-member-type-assumed",
                "feature.face-operand-fallback-contradicted",
                "feature.extrude-target-disputed",
                "feature.edge-selection-native",
                "feature.edge-operand-unresolved",
                "feature.edge-selection-lost",
                "feature.pattern-instance-search-capped",
                "geometry.brep-blob-undecoded",
                "mesh.container-unjoined",
                "mesh.container-undecoded",
                "mesh.container-missing",
                "mesh.attribute-not-transferred",
                "mesh.guid-bytes-uninterpreted",
                "mesh.split-stream-assumed",
                "mesh.history-window-assumed",
                "mesh.operation-records-native",
                "mesh.face-groups-assumed",
                "mesh.body-words-native",
                "mesh.collection-words-native",
                "mesh.body-bytes-uninterpreted",
                "mesh.body-identity-assumed",
                "mesh.wrapper-bytes-uninterpreted",
                "mesh.owner-bytes-uninterpreted",
                "mesh.scope-prefix-uninterpreted",
                "xref.table-undecoded",
                "xref.placement-undecoded",
                "xref.occurrence-unplaced",
                "xref.placement-identity-assumed",
                "xref.placement-metadata-native",
                "xref.placement-role-unbound",
                "xref.placement-current-pose",
                "xref.current-pose-native",
                "xref.current-pose-ambiguous",
                "assembly.local-occurrence-current-pose",
                "assembly.local-occurrence-current-pose-native",
                "assembly.local-occurrence-current-pose-ambiguous",
                "assembly.local-occurrence-path-pose",
                "assembly.local-occurrence-path-pose-unresolved",
                "assembly.local-occurrence-path-pose-disagrees",
                "assembly.local-occurrence-path-pose-inferred",
                "xref.contained-pose",
                "xref.contained-pose-disagrees",
                "xref.contained-pose-native",
                "xref.owned-pose",
                "xref.owned-pose-disagrees",
                "xref.owned-pose-native",
                "xref.placement-metadata-missing",
                "xref.placement-superseded",
                "xref.owner-unresolved",
                "assembly.joint-placed-occurrence-unsolved",
                "mesh.vertex-precision-reduced",
                "design.bodyless-carrier",
                "assembly.components-external",
                "geometry.nurbs-surface-carrier",
                "geometry.sketch-region-body-omitted",
                "geometry.nurbs-curve-carrier",
                "geometry.face-surface-dangling",
                "geometry.surface-shape-not-decoded",
                "geometry.mesh-surface-sentinel",
                "geometry.procedural-curve-undecoded",
                "geometry.vertex-tolerance-widened",
                "geometry.spring-support-pcurve-unresolved",
                "geometry.procedural-support-chart-unresolved",
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
                "tspline.radial-map-unresolved",
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
                "source.unindexed-records-retained",
                "design.intent-undecoded",
                "design.body-display-name-ambiguous",
                "work-plane.display-name-ambiguous",
                "work-plane.display-name-undecoded",
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
