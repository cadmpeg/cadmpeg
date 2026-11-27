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
    /// The `AssetSettings` direction pair is neither the Y-up nor the Z-up
    /// form, or its two copies differ; the document states no up axis.
    SettingsUpAxisUnrecognized,
    /// Record 0 is present in a design stream but is not a whole
    /// `AssetSettings` record of the 494-byte type-17 form, or holds another
    /// key; the document states no up axis.
    SettingsUpAxisRecordUnreadable,
    /// Saved scene visibility names retain their native display state.
    SceneVisibilityNative,
    DraftPartingControlsPartial,
    DraftNeutralAnchorAssumed,
    DraftSlopeOutOfRange,
    BaseFaceRegionsNative,
    BaseFaceSourceCarrierUnresolved,
    BaseFaceThicknessUnresolved,
    BaseFaceMaterialSideUnresolved,
    BaseFaceResultChoiceUnresolved,
    ContourVectorOwnerDistanceRoleAssumed,
    ContourProfileNative,
    ContourDefaultSideOneLeftAssumed,
    ContourEnvelopeUnresolved,
    ContourOwnerVectorDirectionAssumed,
    ContourOperationFourBodyAssumed,
    ContourResultUnresolved,
    ContourSettingsRuleReferenceAssumed,
    ContourRuleFirstRadiusAssumed,
    ContourMaterialScalarFactorAssumed,
    ContourInlineThicknessAssumed,
    ContourRadiusDatabaseUnitAssumed,
    ContourDistanceDatabaseUnitAssumed,
    ContourCachedOwnerDistanceAssumed,
    ContourSingleDistanceOneSidedAssumed,
    ContourDistanceUnresolved,
    ContourFirstRadiusOverrideAssumed,
    ContourDirectionUnresolved,
    ContourThicknessUnresolved,
    ContourRadiusUnresolved,
    ContourRuleFactorUnresolved,
    UnfoldAllBendsFlagAssumed,
    EdgeReferenceEnvelopeUnresolved,
    EdgeParameterOwnerBacklinkAssumed,
    EdgeCachedParameterAssumed,
    EdgeParameterUnitAssumed,
    EdgeParameterUnresolved,
    EdgeStoredRadiusInsideAssumed,
    EdgeRadiusUnresolved,
    EdgeSelectionNative,
    EdgeBoundaryReferenceNative,
    EdgeBoundaryScopeReferenceAssumed,
    EdgeFullWidthAssumed,
    EdgeSignedOffsetsAssumed,
    EdgeDatumUnresolved,
    EdgePositionUnresolved,
    EdgeWidthUnresolved,
    EdgeHeightTargetNative,
    EdgeHeightTargetScopeAssumed,
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
    /// A version-four operation table is applied with its path-ordinal value read as leaving the pose unchanged.
    OperationPosePathOrdinalAssumed,
    /// A version-two or version-three operation table is applied with the pose semantics of version one.
    OperationPoseVersionAssumed,
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
    CylinderLocalFrameAssumed,
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
    /// Planar sketch-curve tails with two opaque bytes read the byte before
    /// the u32 lane as the tail marker.
    SketchCurveTailMarkerAssumed,
    /// Planar sketch-curve tails with two opaque bytes read a u32 lane of `1`
    /// as construction geometry.
    SketchCurveTailConstructionAssumed,
    /// A Chamfer's non-face group is read as its edge group despite its role.
    ChamferEdgeRoleAssumed,
    /// A fixed Chamfer scalar is applied to the face-role groups beside the
    /// scope's one edge-role group (DR-891).
    ChamferFixedFaceGroupAssumed,
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
    CoilShortControlsAssumed,
    CoilTargetUnresolved,
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
    /// A body-owning definition with no placed occurrence is placed once below
    /// the document root at its stored body frames.
    StoredFrameOccurrence,
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
    /// A sketch container's entity header has no recognised form; its visibility and name stay unread.
    SketchContainerHeaderUnrecognized,
    /// Sketch geometry backlinks name a sketch geometry group whose member run omits the record.
    SketchGeometryGroupMemberUnlisted,
    SketchGeometryGroupMemberUnbacklinked,
    /// Unresolved `Component Insert` occurrences whose placement has no finite millimetre transform.
    ComponentInsertUnplaced,
    /// Unresolved `Component Insert` occurrences dropped because no authored timeline order tells
    /// top-level inserts from internal operands.
    ComponentInsertOrderUnknown,
    ComponentInsertContainerAssumed,
    ComponentInsertLegacyFrameAssumed,
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
    /// A hidden path retains carriers without neutral occurrence bindings.
    OccurrenceVisibilityNative,
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
    /// An `Assemble` copy differs from the solved `JointOrigin` carrier matrix.
    AssemblyJointOriginEnvelopeMatrixDiscarded,
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
    /// A joint of an archive member names an operand of the member's root
    /// through one placement, so the other placements of the member do not
    /// state it.
    AssemblyMemberJointPlacementBound,
    AssemblyJointSourceBindingAssumed,
    AssemblyConnectorOperandsAssumed,
    AssemblyMotionBindingAssumed,
    AssemblyAdditionalOwnerMotionAssumed,
    AssemblyExtendedTailAssumed,
    AssemblyExtendedTypedTailAssumed,
    /// A joint operand path continues inside an unmerged inserted document; the
    /// remaining keys stay a nested occurrence path.
    AssemblyOperandNestedPathRetained,
    /// A projected joint reads an occurrence path with a nonzero identity-pair
    /// word as though the word were zero.
    AssemblyOperandPathWordAssumed,
    /// An F3Z drawing root was omitted while its derived 3D model transferred.
    DrawingDocumentOmitted,
    /// ASM history estimated retained size exceeded the session's remaining retained-byte allowance.
    HistoryBindingBudgetExceeded,
    /// ASM history topology binding exceeded its local work cap.
    HistoryBindingWorkExceeded,
    /// A Design recipe side-sequence search exceeded its local work cap.
    RecipeSideSearchBudgetExceeded,
    FaceRecipeMatchWorkExceeded,
    /// Ordering one owner scope's Design parameters exceeded its local work cap.
    ParameterOrderBudgetExceeded,
    /// An ASM history span stays opaque because record framing failed.
    HistoryRecordFramingFailed,
    /// Design body-map pairs do not resolve to a body in the named BREP blob.
    DesignBodyBindingUnresolved,
    /// Design body-map pairs name a body outside the current model.
    HistoryOnlyBodyOmitted,
    /// Snapshot body maps select the model bodies of a Design stream whose
    /// modern body maps are all empty beside a flagged `Base Feature` frame.
    FlaggedBaseFeatureBodySelected,
    /// Reference-image timeline objects retain native Canvas records.
    ReferenceImageNativeRetained,
    ReferenceImageFlagAssumed,
    /// Decal timeline objects retain native image and mapping records.
    DecalNativeRetained,
    DecalMappingAssumed,
    /// A Decal with a decoded placement record keeps its frame in the native record.
    DecalPlacementNative,
    /// A Decal placement record is read in a trailing-reference layout other than the one-byte separator and four-byte gap.
    DecalPlacementLayoutAssumed,
    /// A Decal placement record stores a normal outside the direction of `U x V` and its opposite.
    DecalPlacementNormalAssumed,
    DecalTargetUnresolved,
    DecalImageUnresolved,
    DecalDefinitionUnresolved,
    /// A body-override appearance replaced a different appearance that the body's material assignment names; the override is assumed to take precedence.
    BodyOverridePrecedenceAssumed,
    /// A later body-override appearance naming a different appearance than the body's first override is not bound; the first override is assumed to bind.
    BodyOverrideSuperseded,
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
    FeatureMeshTessellateScopeUnparsed,
    FeatureMeshTessellateFieldsUninterpreted,
    FeatureMeshTessellateSourceUnresolved,
    FeatureMeshPlaneCutScopeUnparsed,
    FeatureMeshPlaneCutTailPartial,
    FeatureMeshPlaneCutPlanePartial,
    FeatureMeshPlaneCutSelectorUnruled,
    FeatureMeshPlaneCutFieldsUninterpreted,
    FeatureMeshPlaneCutSourceUnresolved,
    FeatureMeshPlaneCutSelectionUnresolved,
    FeatureMeshPlaneCutMembershipAssumed,
    FeatureMeshPlaneCutMatrixOrientationAssumed,
    FeatureMeshPlaneCutUnitsAssumed,
    FeatureMeshPlaneCutModelSpaceAssumed,
    FeatureMeshPlaneCutRetainedSideAssumed,
    FeatureMeshPlaneCutSplitFacesAssumed,
    FeatureMeshPlaneCutUniformFillAssumed,
    FeatureMeshCombineScopeUnparsed,
    FeatureMeshCombineTailPartial,
    FeatureMeshCombineSelectorUnruled,
    FeatureMeshCombineFieldsUninterpreted,
    FeatureMeshCombineSourceUnresolved,
    FeatureMeshCombineSelectionUnresolved,
    FeatureMeshCombineMergeAssumed,
    FeatureMeshCombineLegacyBooleanAssumed,
    FeatureMeshCombineKeepToolsAssumedFalse,
    FeatureMeshCombineDestinationAssumedTargetComponent,
    FeatureMeshCombineKeepToolsUnresolved,
    FeatureMeshCombineDestinationUnresolved,
    FeatureMeshConversionTailPartial,
    FeatureMeshConversionMethodUnresolved,
    FeatureMeshConversionMethodAssumed,
    FeatureMeshConversionEnvelopeAssumed,
    FeatureMeshConversionSourceUnresolved,
    FeatureMeshConversionOutputUnresolved,
    FeatureMeshSeparateScopeUnparsed,
    FeatureMeshSeparateTailPartial,
    FeatureMeshSeparateReferenceUnresolved,
    FeatureMeshSeparateReferenceMismatch,
    FeatureMeshSeparateSelectorUnruled,
    FeatureMeshSeparateShellsMethodAssumed,
    FeatureMeshSeparateFaceGroupsMethodAssumed,
    FeatureMeshSeparateAllGroupsAssumed,
    FeatureMeshSeparateSelectedGroupsAssumed,
    FeatureMeshSeparateMultipleBodiesCarrierAssumed,
    FeatureMeshSeparateMultipleBodiesPolarityAssumed,
    FeatureMeshSeparateKeepBodyCarrierAssumed,
    FeatureMeshSeparateKeepBodyPolarityAssumed,
    FeatureMeshSeparatePrefixAssumed,
    FeatureMeshSeparateOutputRoleAssumed,
    FeatureMeshSeparateOutputUnresolved,
    FeatureMeshSeparateSourceMembershipAssumed,
    FeatureMeshSeparateSourceUnresolved,
    FeatureMeshSeparateSourceFieldsUninterpreted,
    FeatureMeshSeparateReferenceRolesAssumed,
    FeatureMeshSeparateMembershipUnresolved,
    /// A historical Thread selection has no complete proof of its live faces.
    ThreadLiveFaceUnproven,
    /// A Thread scope without the extended prefix stores modeled flag `0`, which is read as a cosmetic thread.
    ThreadModeledFlagAssumed,
    /// An extended Thread prefix stores no face entries for a thread whose
    /// modeled flag is `1`; the thread is read as modeled.
    ThreadModeledEntriesAbsent,
    /// A Thread scope has no admitted Thread construction; the feature stays
    /// an unresolved Thread.
    ThreadConstructionUnresolved,
    /// A Thread start offset is retained by its native parameter owner.
    ThreadOffsetNative,
    /// A Thread profile angle other than 60 degrees is retained by the native
    /// Thread construction.
    ThreadAngleNative,
    /// Thread side, flank angle, exact groove profile, and hand outside the extended standard form stay native.
    ThreadDetailNative,
    /// A Hole-owned Thread retains its specification in native records.
    ThreadHoleNative,
    /// The nominal diameter of a Thread with a fractional nominal size is
    /// read as its stored major diameter.
    ThreadFractionalSizeDiameterAssumed,
    /// A stored-body operation has an empty proven body set.
    FeatureStoredBodiesEmpty,
    /// A decoded body snapshot supplies no construction recipe.
    FeatureStoredBodiesRecipeAbsent,
    FeatureHistoryBodyBounded,
    FeatureHistoryBodyUndecoded,
    FeatureHistoryBodyConsumedWithheld,
    /// Later body changes retain a native operation after the linked feature history.
    FeatureBodyHistoryIncomplete,
    FeatureBodyHistoryOwnerUnresolved,
    /// No single decoded history holds a body, so changes after its last feature are not checked.
    FeatureBodyHistoryUnread,
    FeatureOwnedBodyPairingAssumed,
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
    /// A compact feature seed uses its group selection after scope resolution fails.
    FeaturePatternFeatureSeedSubstituted,
    /// A Revolve plane target is read from the `WorkPlane` scope at the record after its selection identity.
    FeatureRevolvePlaneWorkPlaneAssumed,
    /// A circular pattern's symmetric distribution has no circular pattern field.
    FeatureCircularPatternPlacementNotTransferred,
    /// A circular pattern projects with its suppressed instances because their
    /// instance order is unread.
    FeatureCircularPatternSuppressionNotTransferred,
    /// A partial-turn circular pattern without a readable symmetric byte is read as one-sided.
    FeatureCircularPatternDistributionAssumed,
    /// A circular pattern assumes equal spacing because its spacing control is unread.
    FeatureCircularPatternControlsAssumed,
    /// A rectangular-pattern payload's seed kind or body references disagree
    /// with its seed group, whose seeds are kept.
    FeaturePatternPayloadSeedUnconfirmed,
    /// A rectangular-pattern direction record of distance type 10 without a
    /// readable distribution block is read as a seed-to-final-instance extent.
    FeatureRectangularPatternDistanceFormAssumed,
    /// The symmetric flag of an inactive rectangular-pattern direction is
    /// dropped.
    FeatureRectangularPatternInactiveSymmetricDropped,
    /// The record that a rectangular-pattern payload names as its
    /// U-direction record is read as the record of an inactive direction one.
    FeatureRectangularPatternDirectionOneRecordAssumed,
    /// A linear rectangular-pattern direction has no admitted direction
    /// record and no generated-instance run.
    FeatureRectangularPatternDirectionUnresolved,
    /// The auxiliary selection links of a 215- or 216-byte direction remain native.
    FeatureRectangularPatternDirectionLinksNative,
    /// A malformed rectangular-pattern construction remains native.
    FeatureRectangularPatternConstructionNative,
    /// A rectangular pattern or an unproved input retains its native record.
    FeatureRectangularPatternDesignNative,
    /// A single linear rectangular-pattern direction record disagrees with
    /// the generated-instance run; the run direction is kept.
    FeatureRectangularPatternDirectionRecordDisagrees,
    /// A feature scope has no complete unique ASM output transition interval.
    FeatureOutputStateChainUnbound,
    /// An affected body slot has no unique active body in the selected BREP.
    FeatureOutputBodyUnbound,
    /// A pattern result state holds a generated solid body that no model body joins.
    FeaturePatternCopyBodyOmitted,
    /// A derived base scope stores a copy of a source and frame that the archive does not contain.
    FeatureDerivedBaseUnresolved,
    /// A Pipe section is inferred as a circle from its result faces.
    FeaturePipeCircularSectionInferred,
    /// A Pipe with a point-selection pair whose fractions both equal one is assumed to sweep its whole path group.
    FeaturePipeWholePathExtentAssumed,
    /// A Pipe section without an unchanged new-body result is assumed to be a circle.
    FeaturePipeCircularSectionAssumed,
    /// A Pipe with one unit and one zero fraction is assumed to place its section where the unit fraction starts.
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
    SheetMetalRuleControlsNative,
    SheetMetalProfileNative,
    BaseFlangeProfileUnresolved,
    BaseFlangeThicknessUnresolved,
    BaseFlangeSideUnresolved,
    /// Base flanges with a read operation frame project the Forward material side, which the frame does not store.
    BaseFlangeSideAssumed,
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
    /// A face-point Hole carrier and its point-data records place the Hole at different points, and no admitted rule selects one.
    HolePlacementPointUnselected,
    FeaturePointDirectionInputsUnrepresented,
    HoleReferenceDimensionsNative,
    HoleParametersIndependentAssumed,
    HoleShapePartialAssumed,
    HoleExtentLeadingWordAssumed,
    /// A tapped Hole keeps its stored diameter instead of the bore of its thread size.
    HoleTappedDiameterStored,
    /// A Hole with an absent stored diameter uses its Thread minor diameter as the bore.
    HoleThreadMinorDiameterAssumed,
    /// A taper-tapped Hole keeps a cylindrical bore of its stored diameter instead of the tapered bore of its thread size.
    HoleTaperTappedBoreCylindrical,
    /// A Base Feature whose marked null reference stores flag `1` reads its result runs as the ordinary result set.
    FeatureBaseFeatureNullFlagAssumed,
    /// A Base Feature suffix result-body frame with more than one body reads its body, repeated, and result entries as three consecutive runs.
    FeatureBaseFeatureSuffixRunOrderAssumed,
    /// A compact Fillet flag run or set frame stores nonzero bytes in its padding positions, which are read as padding.
    FeatureFilletPaddingAssumed,
    /// Stored variable Fillet midpoint pairs are read in increasing edge position.
    FeatureFilletMidpointOrderAssumed,
    /// A Fillet set uses an inferred input assignment or retains an unresolved input.
    FeatureFilletSetInputUnresolved,
    /// An explicit Fillet set references an unresolved edge operand group.
    FeatureFilletSetGroupUnresolved,
    HoleStandardSizingAssumed,
    /// A clearance Hole's specification takes fixed values for the fields it does not store.
    HoleClearanceFieldsAssumed,
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
    /// A duplicate scope envelope that differs from the kept envelope in
    /// payload or history state was superseded by the primary-indexed
    /// envelope, or by the later envelope when the two differ only in their
    /// tail flags.
    FeatureScopeEnvelopeSuperseded,
    /// Decoded Design parameters have no neutral parameter.
    ParameterUnprojected,
    /// Typed compact parameter frames match no parameter grammar.
    ParameterRecordUnparsed,
    /// Decoded parameters set an uninterpreted flag in the expression trailer.
    ParameterTrailerFlagUninterpreted,
    /// Decoded parameters set the uninterpreted header flag at offset 21.
    ParameterHeaderFlagUninterpreted,
    /// Decoded parameters set the flag byte of a nine-byte or ten-byte
    /// expression trailer that the decoder reads as the driven flag.
    ParameterDrivenFlagAssumed,
    /// Decoded parameter owners store a set final flag byte in the compact trailer.
    ParameterOwnerTrailerFlagUninterpreted,
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
    /// Parameters of a document without a design scope of its own stay v10 parameters.
    ParameterReferencedDesignUnscoped,
    /// Model parameters whose owner feature has no timeline item stay v10 parameters.
    ParameterOwnerOutsideTimeline,
    /// Model parameters whose owner is unresolved stay v10 parameters.
    ParameterOwnerUnresolvedOmitted,
    /// Parameters with a blank name stay v10 parameters.
    ParameterNameBlank,
    /// Configuration rows of a document without a design scope of its own stay v10 configurations.
    ConfigurationReferencedDesignUnscoped,
    /// Configuration cells that set a parameter or feature the design does not state.
    ConfigurationOverrideUnstated,
    /// Configuration parameter cells whose cached value the design does not state.
    ConfigurationOverrideValueUnprojected,
    /// Configuration rows of a scope whose active row does not resolve stay v10 configurations.
    ConfigurationScopeWithoutActiveRow,
    /// Configuration properties the design does not state.
    ConfigurationPropertyUnprojected,
    /// The rollback marker of a timeline is not read, or an unread item follows a rolled-back item; the marker is stated at its most likely position.
    HistoryRollbackUnread,
    /// An unread timeline item precedes the first rolled-back item; the rollback marker can stand before it.
    HistoryRollbackBoundUnread,
    /// A coordinate frame retains its source construction references and expressions.
    CoordinateSystemConstructionNative,
    /// A coordinate frame transfers without its source construction references and expressions.
    CoordinateSystemConstructionDropped,
    /// A coordinate-system scope has an unresolved solved frame.
    CoordinateSystemFrameUnresolved,
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
    /// A treatment group takes the transition edges that the other groups of
    /// its scope do not take.
    HistoricalEdgeSelectionLeftoverAssumed,
    /// A treatment group whose members' first sides name three faces takes
    /// the one distinct assignment of edges that two of each member's faces
    /// share.
    HistoricalEdgeSelectionFirstSidePairAssumed,
    /// A Chamfer face member keeps a loop edge of its face whose continuity
    /// with the adjacent face is undetermined, with the assumption that the
    /// edge is sharp.
    HistoricalEdgeContinuityAssumedSharp,
    /// A fillet with no bound history takes the one history, among several
    /// that hold its state pair, in which every edge group resolves.
    HistoricalEdgeHistoryInferred,
    /// A feature with no bound history, whose state pair several histories
    /// hold, takes the one history of the BREP blob that holds its outputs.
    HistoricalOutputBlobHistoryInferred,
    /// A scope takes the unique history whose blob agrees with its edge recipe faces.
    HistoricalEdgeRecipeHistoryInferred,
    /// A body-recipe operand whose state pair several histories hold takes the
    /// unique history whose BREP holds every candidate face of the operand.
    HistoricalBodyRecipeHistoryInferred,
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
    /// One indexed sketch curve retains an unread geometry member.
    SketchCurveUndecoded,
    /// One indexed sketch curve record failed decoding and is given up.
    SketchCurveRecordUndecoded,
    /// One sketch surface failed decoding.
    SketchSurfaceUndecoded,
    /// One indexed sketch text record failed decoding.
    SketchTextUndecoded,
    /// One sketch relation failed decoding.
    SketchRelationUndecoded,
    /// One sketch folder has an unread member sequence.
    SketchFolderUndecoded,
    /// One sketch ordinal collection has an unread member sequence.
    SketchOrdinalCollectionUndecoded,
    /// One sketch container has an unread visibility member.
    SketchContainerUndecoded,
    /// One sketch geometry group has an unread member sequence.
    SketchGeometryGroupUndecoded,
    /// One feature-timeline record failed decoding.
    FeatureTimelineUndecoded,
    /// One bounded Design primary record has an unread header.
    PrimaryRecordUndecoded,
    /// Configuration projection failed while its documents remain native.
    ConfigurationProjectionNative,
    /// Multiple sketches claim one geometry record.
    SketchOwnerConflict,
    /// One sketch relation has an unresolved owning Design entity.
    SketchRelationOwnerUnresolved,
    /// One configuration document failed decoding.
    ConfigurationDocumentUndecoded,
    /// One sheet-metal settings record failed decoding.
    BendSettingsUndecoded,
    /// One cached body bound failed decoding.
    DesignBodyBoundsUndecoded,
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
    /// An extrude selects only sketch text and its cut was not projected.
    FeatureExtrudeSketchTextSkipped,
    /// Multiple same-sketch region operands are composed as the union of their regions.
    FeatureExtrudeProfileGroupUnionAssumed,
    /// An Extrude profile names a Sketch entity that the Design stream does not register.
    FeatureExtrudeProfileSketchAbsent,
    FeatureExtrudeSurfaceProfileAssumed,
    FeatureExtrudeSymmetricDistanceAssumed,
    FeatureExtrudeWallThicknessNative,
    FeatureExtrudeAuxiliarySelectionNative,
    FeatureExtrudeFaceControlAssumed,
    FeatureExtrudeSketchPlacementNative,
    FeatureExtrudeTargetOptionAssumed,
    FeatureExtrudeOpaqueDistanceWordAssumed,
    FeatureExtrudeStartFaceAbsentAssumed,
    FeatureExtrudeStartFaceUnresolved,
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
    /// An Extrude face profile stays native because the face its selection names is on a curved carrier.
    FeatureExtrudeProfileFaceCurved,
    FeatureSketchProfileFlagsAssumed,
    /// A scope tail sets the third state byte; the scope is read as an active feature with no evaluated result.
    FeatureUnevaluatedStateAssumed,
    /// A scope tail before the timeline marker stores the no-result flag and no suppression state byte; the scope is read as an active feature with no current result.
    FeatureNoResultActiveAssumed,
    /// A scope tail stores a state byte above `1`; the byte is read as set.
    FeatureStateByteAboveOne,
    /// Extrude profile selections skipped the sketch arrangement search at a local cap.
    FeatureProfileArrangementCapped,
    FeatureProfileJoinEnvelopeCapped,
    FeatureProfileCrossingRunsCapped,
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
    FeatureProfileCoincidentBoundaryAssumed,
    FeatureProfileCrossingRunsAssumed,
    FeatureProfilePartialEntryAssumed,
    /// Extrude profile selections took regions from the faces a cut changed, at one point per region.
    FeatureProfileCutRegionsAssumed,
    /// A stored transition certifies regions with stale curve identities.
    FeatureProfileRegionIdentitiesStale,
    /// The status record of a typed profile member lists a
    /// `PROFILE_REFERENCE_LOST` record. The member contributes a region only
    /// when its complete group resolves.
    FeatureProfileMemberReferenceLost,
    /// Extrude profile picks that exceed the carried regions of their Sketch
    /// name its text on the reading that each pick is one glyph.
    FeatureProfileTextGlyphsAssumed,
    /// Extrude profile picks that exceed the carried regions of their Sketch
    /// lie in its text, and the profile names none of their glyphs.
    FeatureProfileTextGlyphsNative,
    /// Extrude profile selections assumed that a curve outside the
    /// arrangement makes no island in a selected face.
    FeatureProfileIslandAbsenceAssumed,
    /// Extrude profile selections read counted member pieces over the
    /// partition by the curves of each member list.
    FeatureProfilePiecePartitionAssumed,
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
    /// Extrude profile selections assume the smallest concentric sketch-circle ring set that
    /// covers a cap bounded by a preceding body cylinder.
    FeatureProfileBodyCircleRingsAssumed,
    /// Extrude profile selections stated the stored spatial profiles of a
    /// spatial Sketch frame that the Extrude transition did not certify.
    FeatureSpatialProfileTransitionAssumed,
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
    FeatureFormSourceUnavailable,
    FeatureFormDefinitionUnavailable,
    FeatureFormMembershipUnresolved,
    FeatureFormLegacySerializerMembershipAssumed,
    FeatureFormLegacySerializerOrderAssumed,
    FeatureFormCageBindingAmbiguous,
    FeatureFormBodyReferencesPartialAssumed,
    FeatureFormBodyRegistrationUnsupported,
    FeatureFormReferenceTypeConflict,
    FeatureFormForeignReferenceUnresolved,
    FeatureFormTrackingOwnerConflict,
    FeatureFormTrackingRunUnresolved,
    FeatureFormTrackingFixedSlotsAssumed,
    FeatureFormSerializerReferenceUnresolved,
    FeatureFormMembershipConflict,
    /// A legacy Scale form is selected by the frame length of its English
    /// spelling; its payload length matches no form.
    FeatureScaleFrameLengthFormAssumed,
    FeatureScaleMatrixWordAssumed,
    FeatureScaleUnlistedGroupsAssumed,
    FeatureWorkAxisRuleUnassigned,
    FeatureWorkAxisEndpointToleranceAssumed,
    FeatureWorkAxisNonzeroParametersSolvedAssumed,
    FeatureWorkAxisPartialCarrierSolvedAssumed,
    FeatureWorkAxisTwoPointOperandsUnresolvedSolvedAssumed,
    FeatureWorkAxisTwoPointConflictSolvedAssumed,
    FeatureWorkAxisGeometryUnrepresentable,
    FeatureWorkAxisBindingUnresolved,
    FeatureWorkAxisLeadingBlockLayoutAssumed,
    FeatureWorkAxisUnsupportedVersionLegacyLayoutAssumed,
    FeatureWorkAxisLegacyLayoutAssumed,
    FeatureWorkAxisCandidatesUnresolved,
    /// Null work-axis input references state no construction input.
    FeatureWorkAxisInputNull,
    /// A work axis whose stored line was read only in part states no line.
    FeatureWorkAxisPartialLineNative,
    /// Feature body selections retain native identities; no unique body.
    FeatureBodySelectionNative,
    /// A pattern seed or `RemoveBody` selection of omitted bodies has no
    /// resolved ASM history: its producer is the one inserting state of any
    /// history, or the selection keeps its native slots.
    FeatureGeneratedBodyHistoryUnresolved,
    /// Reverse Normal body-group members lack the whole-body record type.
    FeatureReverseNormalBodyUnverified,
    /// A Reverse Normal selection stays in its input state: its bound
    /// history, the model bodies of its slots in that history's BREP source,
    /// or the generating features of absent slots are unresolved.
    FeatureReverseNormalBodyProducerUnresolved,
    /// A linked Reverse Normal scope takes no timeline position: no timeline
    /// item carries its link GUID.
    FeatureReverseNormalLinkUnplaced,
    /// A linked Reverse Normal scope takes no timeline position: several
    /// timeline items carry its link GUID.
    FeatureReverseNormalLinkShared,
    /// A linked Reverse Normal scope takes its timeline position by record
    /// order among the other linked scopes of its timeline item.
    FeatureReverseNormalLinkOrderAssumed,
    /// A Combine operand resolved in its input state names no model body and
    /// no generating feature, so it keeps its input-state bodies.
    FeatureCombineBodyProducerUnresolved,
    /// A generated Combine operand slot names the feature that inserts it:
    /// the state that last updates the slot before the Combine has no unique
    /// earlier scope.
    FeatureCombineOperandWriterAssumed,
    /// A Combine tool in an inserted design binds to that design's final
    /// body in the inserting occurrence's frame.
    FeatureCombineToolExternalFinalStateAssumed,
    FeatureSilhouetteSplitPrefixUnparsed,
    FeatureSilhouetteSplitModeUnresolved,
    FeatureSilhouetteSplitDirectionUnresolved,
    FeatureSilhouetteSplitTargetUnresolved,
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
    ReplaceFaceFrameAssumed,
    ReplaceFaceGroupsAggregatedAssumed,
    ReplaceFaceOverlapUnresolved,
    ReplaceFaceSelectionAssumed,
    ReplaceFaceTangentChainAssumed,
    ReplaceFaceSupportAssumed,
    ReplaceFaceIdentityAssumed,
    ReplaceFaceFlaggedPrefixWordAssumed,
    SurfaceUntrimFrameAssumed,
    SurfaceUntrimGroupSelectionAssumed,
    SurfaceUntrimStoredLoopGroupsAssumed,
    SurfaceUntrimLoopGroupsAggregatedAssumed,
    SurfaceUntrimExtensionUnresolved,
    SurfaceUntrimExtensionUnrepresentable,
    SurfaceUntrimNegativeExtensionAssumed,
    SurfaceUntrimPaddingAssumed,
    SurfaceUntrimTrailingIdentityAssumed,
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
    /// A tool of an operand-list Combine names a body that another component
    /// builds, and the generating feature is not proven.
    FeatureCombineToolBodyUnbound,
    /// The legacy cross-document tool selectors of a Combine bind no single
    /// occurrence and distinct bodies of an archive member, or the operands
    /// refuse that binding, so the tools stay local.
    FeatureCombineToolExternalBodyUnbound,
    /// A generated tool body of an operand-list Combine keeps its body-map
    /// pair identity: the generating feature has no one history state, or its
    /// creating state inserts no one body slot.
    FeatureCombineToolResultStateUnbound,
    /// The browser node of an operand-list Combine operand names no distinct
    /// body of the preceding history state, and the body recipes of the scope
    /// bind no body for it, so that operand stays native.
    FeatureCombineOperandBodyUnbound,
    /// The body recipe of an operand-list Combine names model bodies for its
    /// tools, and these bodies replace the tools that the browser-node slots
    /// bind.
    FeatureCombineToolRecipeReplacementAssumed,
    /// A Combine frame that matches the fixed fields of the operand-list
    /// prologue, and whose lists do not complete that prologue, is read in the
    /// standard prologue.
    FeatureCombinePrologueAssumed,
    /// An operand-list Combine prologue stores an operation-entry form other
    /// than one entry or a null-first two-entry pair; its tool list is read
    /// after the last stored entry.
    FeatureCombineOperationEntriesAssumed,
    FeatureCombineKeptToolsCutAssumed,
    /// A Combine whose lists name its operands, whose other fields contradict
    /// the operand-list prologue or cannot be checked, and which no prologue
    /// reads stays native.
    FeatureCombineOperandListRefused,
    /// A Combine with a companion `RemoveBody` scope reads every tool as
    /// removed, though the companion body selections do not name exactly the
    /// tools that its consumed-body list omits.
    FeatureCombineToolConsumptionAssumed,
    FeatureSplitFaceMethodUnresolved,
    FeatureSplitFaceDirectionUnresolved,
    FeatureSplitFaceGroupsUnresolved,
    FeatureSplitFaceSelectionUnresolved,
    FeatureSplitFaceSelectionProjectionFailed,
    FeatureSplitFaceCarrierTypeAssumed,
    FeatureSplitFaceIndexedOperandAssumed,
    /// Fixed Fillet scalar lanes have no complete group assignment.
    FeatureFixedFilletAssignmentNative,
    #[doc = "A Fillet selection has no decoded radius law or dimension."]
    FeatureFilletRadiusUnresolved,
    #[doc = "Fillet source references do not establish complete dimensional bindings."]
    FeatureFilletReferencesUnresolved,
    #[doc = "A structurally identified full-round Fillet has an unresolved center."]
    FeatureFilletFullRoundCenterUnresolved,
    /// A parameter-owned Fillet scope's set list is not an admitted form; its operand groups have no radius assignment.
    FeatureFilletSetListNative,
    /// Extrude scope retains an unassigned extent payload.
    FeatureExtrudeExtentNative,
    /// An Extrude face extent has fewer termination groups than its side count.
    FeatureExtrudeTerminationMissing,
    /// A marker-one `Revolve` scope has a token-independent frame length outside its documented set.
    FeatureRevolveLayoutAssumed,
    /// A Revolve scope frame matches no admitted Revolve form; its
    /// construction stays undecoded.
    FeatureRevolveFrameUnmatched,
    /// A marked `Move` or `Modify` transform record breaks a type, frame-length, or form-word rule of the rigid-transform record, or a `MoveFace` transform record breaks a frame-length or form-word rule.
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
    /// A new-body `Loft` takes its solid setting from the kinds of its output bodies.
    FeatureLoftSolidInferred,
    /// A `SurfaceExtend` scope projects with an unresolved extension method.
    FeatureSurfaceExtendMethodUnresolved,
    /// A `SurfaceExtend` projects its stored boundary edges as the faces they bound.
    FeatureSurfaceExtendEdgesAsFaces,
    FeatureSplitToolExtensionAssumed,
    FeatureSplitToolExtensionUnresolved,
    FeatureSplitBodyFaceToolGroupAssumed,
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
    /// A rectangular pattern's coincident direction has no linear pattern field.
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
    FeatureShellRemovedFacesAssumed,
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
    /// Selected Boundary Fill cells retain their source records.
    FeatureBoundaryFillCellsNative,
    /// Boundary Fill has no admitted cell selection.
    FeatureBoundaryFillCellsUnresolved,
    /// Boundary Fill stores an unassigned operation word.
    FeatureBoundaryFillOperationUnresolved,
    /// Boundary Fill tool identity is unresolved.
    FeatureBoundaryFillToolsUnresolved,
    /// Boundary Fill Boolean target bodies are unresolved.
    FeatureBoundaryFillTargetsUnresolved,
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
    /// A packed-direction split component flag has no settled meaning; its
    /// payload is read with the `{T: 3}` layout.
    MeshComponentFlagUninterpreted,
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
    XrefLineageDisagrees,
    XrefIdentityBlank,
    XrefArchiveIdentityDisagrees,

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
    /// An object-prelude carrier with a source marker takes its placement from
    /// its closing matrix without a component-path proof; the reference frame
    /// of that matrix is assumed.
    LocalOccurrenceMarkedPreludeAssumed,
    /// An object-prelude carrier without a source marker takes its placement
    /// from its closing matrix in a Design stream whose component-path
    /// carriers all name an expanded occurrence and none names this carrier;
    /// the reference frame of that matrix is assumed.
    LocalOccurrenceObjectPreludeAssumed,
    /// A nested occurrence that only some placements of its definition hold
    /// is owned by the definition, so every placement shows it.
    PlacementChildrenDiffer,
    /// A reference that named one placed instance of a nested occurrence,
    /// whose definition is placed more than once, names the occurrence.
    InstanceReferenceShared,
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
    /// Empty modern model-body maps select no body, and the design declares no
    /// body, sketch entity, or reference image.
    EmptyDesign,
    /// A part design stores mesh geometry containers and no BREP entry, and no mesh body transferred.
    MeshOnlyDesignUntransferred,
    /// An assembly document's geometry lives in external references.
    AssemblyComponentsExternal,
    /// Spline surface records were decoded into NURBS carriers from cache.
    NurbsSurfaceCarrier,
    /// Procedural curve records were decoded into NURBS carriers from cache.
    NurbsCurveCarrier,
    /// Faces rest on spline or procedural surfaces whose shape was not decoded.
    SurfaceShapeNotDecoded,
    /// Faces use zero-payload `mesh_surface` sentinels; exact surfaces are absent.
    MeshSurfaceSentinel,
    /// Sketch-region body records of a keyless BREP blob are not model bodies.
    SketchRegionBodyOmitted,
    /// Edges reference a procedural 3D curve with no decodable B-spline cache.
    ProceduralCurveUndecoded,
    /// Vertex tolerance is widened to cover the ends of its edges' curves:
    /// exact cache endpoints on read, the vertices of a ranged open edge on write.
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
    /// A face states no loop on a plane, cylinder, or cone; the face is
    /// stated partial.
    TopologyFaceBoundaryUnstated,
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
    /// The document was read with a grammar its own declarations do not select.
    SourceDialectUnverified,
    /// Export omits records named by the counted loss message.
    WriterAppearancePropertiesOmitted,
    /// A procedural carrier with no native encoding was written as its solved geometry.
    WriterProceduralReduced,
    /// A pcurve with no native encoding was omitted; the target recomputes it.
    WriterPcurveOmitted,
    /// A subdivision surface was not written.
    WriterSubdOmitted,
    /// A document asset was not written.
    WriterAssetOmitted,
    /// An assembly joint was not written.
    WriterJointOmitted,
    /// A parameter was not written.
    WriterParameterOmitted,
    /// A sketch entity or constraint was not written.
    WriterSketchContentOmitted,
    /// A sketch was written on a plane that stands for its source support.
    WriterSketchSupportSubstituted,
    /// A configuration was not written.
    WriterConfigurationOmitted,
    /// A tessellation was not written.
    WriterTessellationOmitted,
    /// A carrier's source-object association was not written.
    WriterSourceAssociationOmitted,
    /// A body was not written, with the topology and geometry only it uses.
    WriterBodyOmitted,
    /// A face on a carrier F3D has no form for was not written; its body was
    /// written open, without it.
    WriterFaceOmitted,
    /// Geometry was not written as the source states it.
    WriterGeometryOmitted,
    /// An open edge whose source states no range was written over one arc of
    /// a curve that passes one of its vertices twice.
    WriterEdgeRangeAssumed,
    /// A timeline whose rollback the document does not state was written
    /// built through its end, with the current geometry as the state after
    /// its last feature.
    WriterRollbackAssumed,
    /// An appearance was not written.
    WriterAppearanceOmitted,
    /// The history arenas of a document read from CADIR JSON were not
    /// restored from its retained source; the file was written from its
    /// `design` records.
    WriterHistoryUnrestored,
    /// An integer parameter was written as a Fusion user parameter, which
    /// holds a real.
    WriterIntegerAsReal,
    /// A design scope, container, or value of the written timeline was not
    /// written.
    WriterDesignRecordOmitted,
    /// An appearance's Protein asset was written under a GUID the writer
    /// allocated, in place of a missing or shared source GUID.
    WriterAppearanceGuidAllocated,
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
        Self::SettingsUpAxisUnrecognized,
        Self::SettingsUpAxisRecordUnreadable,
        Self::SceneVisibilityNative,
        Self::DraftPartingControlsPartial,
        Self::DraftNeutralAnchorAssumed,
        Self::DraftSlopeOutOfRange,
        Self::BaseFaceRegionsNative,
        Self::BaseFaceSourceCarrierUnresolved,
        Self::BaseFaceThicknessUnresolved,
        Self::BaseFaceMaterialSideUnresolved,
        Self::BaseFaceResultChoiceUnresolved,
        Self::ContourVectorOwnerDistanceRoleAssumed,
        Self::ContourProfileNative,
        Self::ContourDefaultSideOneLeftAssumed,
        Self::ContourEnvelopeUnresolved,
        Self::ContourOwnerVectorDirectionAssumed,
        Self::ContourOperationFourBodyAssumed,
        Self::ContourResultUnresolved,
        Self::ContourSettingsRuleReferenceAssumed,
        Self::ContourRuleFirstRadiusAssumed,
        Self::ContourMaterialScalarFactorAssumed,
        Self::ContourInlineThicknessAssumed,
        Self::ContourRadiusDatabaseUnitAssumed,
        Self::ContourDistanceDatabaseUnitAssumed,
        Self::ContourCachedOwnerDistanceAssumed,
        Self::ContourSingleDistanceOneSidedAssumed,
        Self::ContourDistanceUnresolved,
        Self::ContourFirstRadiusOverrideAssumed,
        Self::ContourDirectionUnresolved,
        Self::ContourThicknessUnresolved,
        Self::ContourRadiusUnresolved,
        Self::ContourRuleFactorUnresolved,
        Self::UnfoldAllBendsFlagAssumed,
        Self::EdgeReferenceEnvelopeUnresolved,
        Self::EdgeParameterOwnerBacklinkAssumed,
        Self::EdgeCachedParameterAssumed,
        Self::EdgeParameterUnitAssumed,
        Self::EdgeParameterUnresolved,
        Self::EdgeStoredRadiusInsideAssumed,
        Self::EdgeRadiusUnresolved,
        Self::EdgeSelectionNative,
        Self::EdgeBoundaryReferenceNative,
        Self::EdgeBoundaryScopeReferenceAssumed,
        Self::EdgeFullWidthAssumed,
        Self::EdgeSignedOffsetsAssumed,
        Self::EdgeDatumUnresolved,
        Self::EdgePositionUnresolved,
        Self::EdgeWidthUnresolved,
        Self::EdgeHeightTargetNative,
        Self::EdgeHeightTargetScopeAssumed,
        Self::UnfoldControlUnresolved,
        Self::UnfoldSelectionSlotsAssumed,
        Self::UnfoldSelectionNative,
        Self::UnfoldEnvelopeUnresolved,
        Self::UnfoldMaterialScalarFactorAssumed,
        Self::UnfoldRuleFactorUnresolved,
        Self::UnfoldSettingsRuleReferenceAssumed,
        Self::RefoldGroupSourceAssumed,
        Self::RefoldSourceUnresolved,
        Self::BendLineOwnerOrderAssumed,
        Self::BendStationaryRoleAssumed,
        Self::BendAngleUnresolved,
        Self::BendRadiusFirstAssumed,
        Self::BendStationarySelectionNative,
        Self::BendRuleRadiusUnresolved,
        Self::BendAngleOwnerBacklinkAssumed,
        Self::BendCachedOwnerAngleAssumed,
        Self::SceneOverrideAmbiguous,
        Self::SceneOverrideUnbound,
        Self::OperationPoseApplied,
        Self::OperationPosePathOrdinalAssumed,
        Self::OperationPoseVersionAssumed,
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
        Self::CylinderLocalFrameAssumed,
        Self::SpherePrefixFramingAssumed,
        Self::SpherePlacementFlagsAssumed,
        Self::SphereLocalMatrixFramingAssumed,
        Self::SphereLocalFrameAssumed,
        Self::SpherePlacementFrameAssumed,
        Self::SphereDiameterOwnerAssumed,
        Self::SphereDiameterSourceAssumed,
        Self::ConstructionTrackingPathUntyped,
        Self::ConstructionClosingFlagAssumed,
        Self::SketchCurveTailMarkerAssumed,
        Self::SketchCurveTailConstructionAssumed,
        Self::ChamferEdgeRoleAssumed,
        Self::ChamferFixedFaceGroupAssumed,
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
        Self::CoilShortControlsAssumed,
        Self::CoilTargetUnresolved,
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
        Self::StoredFrameOccurrence,
        Self::LocalOccurrenceOwnerCycle,
        Self::LocalOccurrenceExpansionLimit,
        Self::FeatureOccurrencePathUnresolved,
        Self::SketchOwnerUnresolved,
        Self::SketchNameComposedFromFeatureOrdinal,
        Self::SketchMembershipConflict,
        Self::SketchFolderClosingReference,
        Self::SketchFolderUnprojected,
        Self::SketchContainerHeaderUnrecognized,
        Self::SketchGeometryGroupMemberUnlisted,
        Self::SketchGeometryGroupMemberUnbacklinked,
        Self::ComponentInsertUnplaced,
        Self::ComponentInsertOrderUnknown,
        Self::ComponentInsertContainerAssumed,
        Self::ComponentInsertLegacyFrameAssumed,
        Self::ComponentDefinitionRevised,
        Self::ComponentNamingSpaceUnbound,
        Self::RootInstanceDefinitionUnbound,
        Self::LocalOccurrenceEnvelopeSuperseded,
        Self::LocalOccurrenceEnvelopeNative,
        Self::OccurrenceVisibilityNative,
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
        Self::AssemblyJointOriginEnvelopeMatrixDiscarded,
        Self::AssemblyRigidGroupFrameUnparsed,
        Self::AssemblyJointMotionUndecoded,
        Self::AssemblyJointRestUnprojected,
        Self::AssemblyJointActivationAssumed,
        Self::AssemblyJointAngularLimitsNative,
        Self::AssemblyOperandPathPairsNative,
        Self::AssemblyRootPathIdentitiesNative,
        Self::AssemblyOperandDescentAmbiguous,
        Self::AssemblyOperandDescentUnresolved,
        Self::AssemblyMemberJointPlacementBound,
        Self::AssemblyJointSourceBindingAssumed,
        Self::AssemblyConnectorOperandsAssumed,
        Self::AssemblyMotionBindingAssumed,
        Self::AssemblyAdditionalOwnerMotionAssumed,
        Self::AssemblyExtendedTailAssumed,
        Self::AssemblyExtendedTypedTailAssumed,
        Self::AssemblyOperandNestedPathRetained,
        Self::AssemblyOperandPathWordAssumed,
        Self::DrawingDocumentOmitted,
        Self::HistoryBindingBudgetExceeded,
        Self::HistoryBindingWorkExceeded,
        Self::RecipeSideSearchBudgetExceeded,
        Self::FaceRecipeMatchWorkExceeded,
        Self::ParameterOrderBudgetExceeded,
        Self::HistoryRecordFramingFailed,
        Self::DesignBodyBindingUnresolved,
        Self::HistoryOnlyBodyOmitted,
        Self::FlaggedBaseFeatureBodySelected,
        Self::ReferenceImageNativeRetained,
        Self::ReferenceImageFlagAssumed,
        Self::DecalNativeRetained,
        Self::DecalMappingAssumed,
        Self::DecalPlacementNative,
        Self::DecalPlacementLayoutAssumed,
        Self::DecalPlacementNormalAssumed,
        Self::DecalTargetUnresolved,
        Self::DecalImageUnresolved,
        Self::DecalDefinitionUnresolved,
        Self::BodyOverridePrecedenceAssumed,
        Self::BodyOverrideSuperseded,
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
        Self::FeatureMeshTessellateScopeUnparsed,
        Self::FeatureMeshTessellateFieldsUninterpreted,
        Self::FeatureMeshTessellateSourceUnresolved,
        Self::FeatureMeshPlaneCutScopeUnparsed,
        Self::FeatureMeshPlaneCutTailPartial,
        Self::FeatureMeshPlaneCutPlanePartial,
        Self::FeatureMeshPlaneCutSelectorUnruled,
        Self::FeatureMeshPlaneCutFieldsUninterpreted,
        Self::FeatureMeshPlaneCutSourceUnresolved,
        Self::FeatureMeshPlaneCutSelectionUnresolved,
        Self::FeatureMeshPlaneCutMembershipAssumed,
        Self::FeatureMeshPlaneCutMatrixOrientationAssumed,
        Self::FeatureMeshPlaneCutUnitsAssumed,
        Self::FeatureMeshPlaneCutModelSpaceAssumed,
        Self::FeatureMeshPlaneCutRetainedSideAssumed,
        Self::FeatureMeshPlaneCutSplitFacesAssumed,
        Self::FeatureMeshPlaneCutUniformFillAssumed,
        Self::FeatureMeshCombineScopeUnparsed,
        Self::FeatureMeshCombineTailPartial,
        Self::FeatureMeshCombineSelectorUnruled,
        Self::FeatureMeshCombineFieldsUninterpreted,
        Self::FeatureMeshCombineSourceUnresolved,
        Self::FeatureMeshCombineSelectionUnresolved,
        Self::FeatureMeshCombineMergeAssumed,
        Self::FeatureMeshCombineLegacyBooleanAssumed,
        Self::FeatureMeshCombineKeepToolsAssumedFalse,
        Self::FeatureMeshCombineDestinationAssumedTargetComponent,
        Self::FeatureMeshCombineKeepToolsUnresolved,
        Self::FeatureMeshCombineDestinationUnresolved,
        Self::FeatureMeshConversionTailPartial,
        Self::FeatureMeshConversionMethodUnresolved,
        Self::FeatureMeshConversionMethodAssumed,
        Self::FeatureMeshConversionEnvelopeAssumed,
        Self::FeatureMeshConversionSourceUnresolved,
        Self::FeatureMeshConversionOutputUnresolved,
        Self::FeatureMeshSeparateScopeUnparsed,
        Self::FeatureMeshSeparateTailPartial,
        Self::FeatureMeshSeparateReferenceUnresolved,
        Self::FeatureMeshSeparateReferenceMismatch,
        Self::FeatureMeshSeparateSelectorUnruled,
        Self::FeatureMeshSeparateShellsMethodAssumed,
        Self::FeatureMeshSeparateFaceGroupsMethodAssumed,
        Self::FeatureMeshSeparateAllGroupsAssumed,
        Self::FeatureMeshSeparateSelectedGroupsAssumed,
        Self::FeatureMeshSeparateMultipleBodiesCarrierAssumed,
        Self::FeatureMeshSeparateMultipleBodiesPolarityAssumed,
        Self::FeatureMeshSeparateKeepBodyCarrierAssumed,
        Self::FeatureMeshSeparateKeepBodyPolarityAssumed,
        Self::FeatureMeshSeparatePrefixAssumed,
        Self::FeatureMeshSeparateOutputRoleAssumed,
        Self::FeatureMeshSeparateOutputUnresolved,
        Self::FeatureMeshSeparateSourceMembershipAssumed,
        Self::FeatureMeshSeparateSourceUnresolved,
        Self::FeatureMeshSeparateSourceFieldsUninterpreted,
        Self::FeatureMeshSeparateReferenceRolesAssumed,
        Self::FeatureMeshSeparateMembershipUnresolved,
        Self::ThreadLiveFaceUnproven,
        Self::ThreadModeledFlagAssumed,
        Self::ThreadModeledEntriesAbsent,
        Self::ThreadConstructionUnresolved,
        Self::ThreadOffsetNative,
        Self::ThreadAngleNative,
        Self::ThreadDetailNative,
        Self::ThreadHoleNative,
        Self::ThreadFractionalSizeDiameterAssumed,
        Self::FeatureStoredBodiesEmpty,
        Self::FeatureStoredBodiesRecipeAbsent,
        Self::FeatureHistoryBodyBounded,
        Self::FeatureHistoryBodyUndecoded,
        Self::FeatureHistoryBodyConsumedWithheld,
        Self::FeatureBodyHistoryIncomplete,
        Self::FeatureBodyHistoryOwnerUnresolved,
        Self::FeatureBodyHistoryUnread,
        Self::FeatureOwnedBodyPairingAssumed,
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
        Self::FeaturePatternFeatureSeedSubstituted,
        Self::FeatureRevolvePlaneWorkPlaneAssumed,
        Self::FeatureCircularPatternPlacementNotTransferred,
        Self::FeatureCircularPatternSuppressionNotTransferred,
        Self::FeatureCircularPatternDistributionAssumed,
        Self::FeatureCircularPatternControlsAssumed,
        Self::FeaturePatternPayloadSeedUnconfirmed,
        Self::FeatureRectangularPatternDistanceFormAssumed,
        Self::FeatureRectangularPatternInactiveSymmetricDropped,
        Self::FeatureRectangularPatternDirectionOneRecordAssumed,
        Self::FeatureRectangularPatternDirectionUnresolved,
        Self::FeatureRectangularPatternDirectionLinksNative,
        Self::FeatureRectangularPatternConstructionNative,
        Self::FeatureRectangularPatternDesignNative,
        Self::FeatureRectangularPatternDirectionRecordDisagrees,
        Self::FeatureOutputStateChainUnbound,
        Self::FeatureOutputBodyUnbound,
        Self::FeaturePatternCopyBodyOmitted,
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
        Self::SheetMetalRuleControlsNative,
        Self::SheetMetalProfileNative,
        Self::BaseFlangeProfileUnresolved,
        Self::BaseFlangeThicknessUnresolved,
        Self::BaseFlangeSideUnresolved,
        Self::BaseFlangeSideAssumed,
        Self::FeatureScopeUnprojected,
        Self::MoveUnprojected,
        Self::ModifyUnprojected,
        Self::HoleExtentUnproven,
        Self::HolePlacementUnproven,
        Self::HolePlacementPointUnselected,
        Self::FeaturePointDirectionInputsUnrepresented,
        Self::HoleReferenceDimensionsNative,
        Self::HoleParametersIndependentAssumed,
        Self::HoleShapePartialAssumed,
        Self::HoleExtentLeadingWordAssumed,
        Self::HoleTappedDiameterStored,
        Self::HoleThreadMinorDiameterAssumed,
        Self::HoleTaperTappedBoreCylindrical,
        Self::FeatureBaseFeatureNullFlagAssumed,
        Self::FeatureBaseFeatureSuffixRunOrderAssumed,
        Self::FeatureFilletPaddingAssumed,
        Self::FeatureFilletMidpointOrderAssumed,
        Self::FeatureFilletSetInputUnresolved,
        Self::FeatureFilletSetGroupUnresolved,
        Self::HoleStandardSizingAssumed,
        Self::HoleClearanceFieldsAssumed,
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
        Self::ParameterHeaderFlagUninterpreted,
        Self::ParameterDrivenFlagAssumed,
        Self::ParameterOwnerTrailerFlagUninterpreted,
        Self::ParameterOwnerUnstored,
        Self::ParameterOwnerUnrecognized,
        Self::ParameterOwnerUnparsed,
        Self::ParameterUnitUntyped,
        Self::MaterialDistanceUnitUntyped,
        Self::FaceAppearanceAmbiguous,
        Self::ParameterExpressionUnbound,
        Self::ParameterReferencedDesignUnscoped,
        Self::ParameterOwnerOutsideTimeline,
        Self::ParameterOwnerUnresolvedOmitted,
        Self::ParameterNameBlank,
        Self::ConfigurationReferencedDesignUnscoped,
        Self::ConfigurationOverrideUnstated,
        Self::ConfigurationOverrideValueUnprojected,
        Self::ConfigurationScopeWithoutActiveRow,
        Self::ConfigurationPropertyUnprojected,
        Self::HistoryRollbackUnread,
        Self::HistoryRollbackBoundUnread,
        Self::CoordinateSystemConstructionNative,
        Self::CoordinateSystemConstructionDropped,
        Self::CoordinateSystemFrameUnresolved,
        Self::HistoryDependencyUnprojected,
        Self::HistoryDependencyAmbiguous,
        Self::HistoricalFaceSelectionUnbound,
        Self::HistoryDeleteFaceMemberOrderAssumed,
        Self::HistoricalEdgeMidpointsUnresolved,
        Self::HistoricalFilletEdgeInferred,
        Self::HistoricalEdgeSelectionInferred,
        Self::HistoricalEdgeSelectionDeletedSetAssumed,
        Self::HistoricalEdgeSelectionLeftoverAssumed,
        Self::HistoricalEdgeSelectionFirstSidePairAssumed,
        Self::HistoricalEdgeContinuityAssumedSharp,
        Self::HistoricalEdgeHistoryInferred,
        Self::HistoricalOutputBlobHistoryInferred,
        Self::HistoricalEdgeRecipeHistoryInferred,
        Self::HistoricalBodyRecipeHistoryInferred,
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
        Self::SketchCurveUndecoded,
        Self::SketchCurveRecordUndecoded,
        Self::SketchSurfaceUndecoded,
        Self::SketchTextUndecoded,
        Self::SketchRelationUndecoded,
        Self::SketchFolderUndecoded,
        Self::SketchOrdinalCollectionUndecoded,
        Self::SketchContainerUndecoded,
        Self::SketchGeometryGroupUndecoded,
        Self::FeatureTimelineUndecoded,
        Self::PrimaryRecordUndecoded,
        Self::ConfigurationProjectionNative,
        Self::SketchOwnerConflict,
        Self::SketchRelationOwnerUnresolved,
        Self::ConfigurationDocumentUndecoded,
        Self::BendSettingsUndecoded,
        Self::DesignBodyBoundsUndecoded,
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
        Self::FeatureExtrudeSketchTextSkipped,
        Self::FeatureExtrudeProfileGroupUnionAssumed,
        Self::FeatureExtrudeProfileSketchAbsent,
        Self::FeatureExtrudeSurfaceProfileAssumed,
        Self::FeatureExtrudeSymmetricDistanceAssumed,
        Self::FeatureExtrudeWallThicknessNative,
        Self::FeatureExtrudeAuxiliarySelectionNative,
        Self::FeatureExtrudeFaceControlAssumed,
        Self::FeatureExtrudeSketchPlacementNative,
        Self::FeatureExtrudeTargetOptionAssumed,
        Self::FeatureExtrudeOpaqueDistanceWordAssumed,
        Self::FeatureExtrudeStartFaceAbsentAssumed,
        Self::FeatureExtrudeStartFaceUnresolved,
        Self::FeatureExtrudeZeroSideTaperIgnored,
        Self::FeatureExtrudeProfileFaceOrderAssumed,
        Self::FeatureExtrudeProfileFaceSearchCapped,
        Self::FeatureExtrudeProfileCapFacesInferred,
        Self::FeatureExtrudeProfileDeletedFacesInferred,
        Self::FeatureExtrudeProfileFaceSetDisputed,
        Self::FeatureExtrudeProfileFaceCurved,
        Self::FeatureSketchProfileFlagsAssumed,
        Self::FeatureUnevaluatedStateAssumed,
        Self::FeatureNoResultActiveAssumed,
        Self::FeatureStateByteAboveOne,
        Self::FeatureProfileArrangementCapped,
        Self::FeatureProfileJoinEnvelopeCapped,
        Self::FeatureProfileCrossingRunsCapped,
        Self::FeatureProfileDensificationCapped,
        Self::FeatureProfileCrossingLoopCapped,
        Self::FeatureProfileCrossingLoopAssumed,
        Self::FeatureProfileCoincidentDirectionAssumed,
        Self::FeatureProfileCoincidentBoundaryAssumed,
        Self::FeatureProfileCrossingRunsAssumed,
        Self::FeatureProfilePartialEntryAssumed,
        Self::FeatureProfileLoopFlagsAssumed,
        Self::FeatureProfileCutRegionsAssumed,
        Self::FeatureProfileRegionIdentitiesStale,
        Self::FeatureProfileMemberReferenceLost,
        Self::FeatureProfileTextGlyphsAssumed,
        Self::FeatureProfileTextGlyphsNative,
        Self::FeatureProfileIslandAbsenceAssumed,
        Self::FeatureProfilePiecePartitionAssumed,
        Self::FeatureProfilePolylineSideAssumed,
        Self::FeatureProfileCapOutlineAssumed,
        Self::FeatureProfileCapCarrierAssumed,
        Self::FeatureProfileHoleBoundaryInferred,
        Self::FeatureProfileRegionHoleInferred,
        Self::FeatureProfileBodyCircleRingsAssumed,
        Self::FeatureSpatialProfileTransitionAssumed,
        Self::FeaturePathSelectionNative,
        Self::FeatureFaceSelectionNative,
        Self::DeleteFaceFrameControlsAssumed,
        Self::DeleteFaceGroupSelectionAssumed,
        Self::SurfaceDeleteFaceWidePrologueAssumed,
        Self::FeatureFaceSelectionActiveSubstituted,
        Self::FeatureSurfaceOffsetGroupOrdinalsAssumed,
        Self::FeatureFormSourceUnavailable,
        Self::FeatureFormDefinitionUnavailable,
        Self::FeatureFormMembershipUnresolved,
        Self::FeatureFormLegacySerializerMembershipAssumed,
        Self::FeatureFormLegacySerializerOrderAssumed,
        Self::FeatureFormCageBindingAmbiguous,
        Self::FeatureFormBodyReferencesPartialAssumed,
        Self::FeatureFormBodyRegistrationUnsupported,
        Self::FeatureFormReferenceTypeConflict,
        Self::FeatureFormForeignReferenceUnresolved,
        Self::FeatureFormTrackingOwnerConflict,
        Self::FeatureFormTrackingRunUnresolved,
        Self::FeatureFormTrackingFixedSlotsAssumed,
        Self::FeatureFormSerializerReferenceUnresolved,
        Self::FeatureFormMembershipConflict,
        Self::FeatureScaleFrameLengthFormAssumed,
        Self::FeatureScaleMatrixWordAssumed,
        Self::FeatureScaleUnlistedGroupsAssumed,
        Self::FeatureWorkAxisRuleUnassigned,
        Self::FeatureWorkAxisEndpointToleranceAssumed,
        Self::FeatureWorkAxisNonzeroParametersSolvedAssumed,
        Self::FeatureWorkAxisPartialCarrierSolvedAssumed,
        Self::FeatureWorkAxisTwoPointOperandsUnresolvedSolvedAssumed,
        Self::FeatureWorkAxisTwoPointConflictSolvedAssumed,
        Self::FeatureWorkAxisGeometryUnrepresentable,
        Self::FeatureWorkAxisBindingUnresolved,
        Self::FeatureWorkAxisLeadingBlockLayoutAssumed,
        Self::FeatureWorkAxisUnsupportedVersionLegacyLayoutAssumed,
        Self::FeatureWorkAxisLegacyLayoutAssumed,
        Self::FeatureWorkAxisCandidatesUnresolved,
        Self::FeatureWorkAxisInputNull,
        Self::FeatureWorkAxisPartialLineNative,
        Self::FeatureBodySelectionNative,
        Self::FeatureGeneratedBodyHistoryUnresolved,
        Self::FeatureReverseNormalBodyUnverified,
        Self::FeatureReverseNormalBodyProducerUnresolved,
        Self::FeatureReverseNormalLinkUnplaced,
        Self::FeatureReverseNormalLinkShared,
        Self::FeatureReverseNormalLinkOrderAssumed,
        Self::FeatureCombineBodyProducerUnresolved,
        Self::FeatureCombineOperandWriterAssumed,
        Self::FeatureCombineToolExternalFinalStateAssumed,
        Self::FeatureSilhouetteSplitPrefixUnparsed,
        Self::FeatureSilhouetteSplitModeUnresolved,
        Self::FeatureSilhouetteSplitDirectionUnresolved,
        Self::FeatureSilhouetteSplitTargetUnresolved,
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
        Self::ReplaceFaceFrameAssumed,
        Self::ReplaceFaceGroupsAggregatedAssumed,
        Self::ReplaceFaceOverlapUnresolved,
        Self::ReplaceFaceSelectionAssumed,
        Self::ReplaceFaceTangentChainAssumed,
        Self::ReplaceFaceSupportAssumed,
        Self::ReplaceFaceIdentityAssumed,
        Self::ReplaceFaceFlaggedPrefixWordAssumed,
        Self::SurfaceUntrimFrameAssumed,
        Self::SurfaceUntrimGroupSelectionAssumed,
        Self::SurfaceUntrimStoredLoopGroupsAssumed,
        Self::SurfaceUntrimLoopGroupsAggregatedAssumed,
        Self::SurfaceUntrimExtensionUnresolved,
        Self::SurfaceUntrimExtensionUnrepresentable,
        Self::SurfaceUntrimNegativeExtensionAssumed,
        Self::SurfaceUntrimPaddingAssumed,
        Self::SurfaceUntrimTrailingIdentityAssumed,
        Self::UnstitchFrameAssumed,
        Self::UnstitchGroupSelectionAssumed,
        Self::UnstitchPrologueControlsAssumed,
        Self::FeatureCombineSelectionNative,
        Self::FeatureCombineDefinitionUnresolved,
        Self::FeatureCombineMiddleListOmittedAssumed,
        Self::FeatureCombineComponentDestinationAssumed,
        Self::FeatureCombineNamedTailPrologueAssumed,
        Self::FeatureCombineToolBodyUnbound,
        Self::FeatureCombineToolExternalBodyUnbound,
        Self::FeatureCombineToolResultStateUnbound,
        Self::FeatureCombineOperandBodyUnbound,
        Self::FeatureCombineToolRecipeReplacementAssumed,
        Self::FeatureCombinePrologueAssumed,
        Self::FeatureCombineOperationEntriesAssumed,
        Self::FeatureCombineKeptToolsCutAssumed,
        Self::FeatureCombineOperandListRefused,
        Self::FeatureCombineToolConsumptionAssumed,
        Self::FeatureSplitFaceMethodUnresolved,
        Self::FeatureSplitFaceDirectionUnresolved,
        Self::FeatureSplitFaceGroupsUnresolved,
        Self::FeatureSplitFaceSelectionUnresolved,
        Self::FeatureSplitFaceSelectionProjectionFailed,
        Self::FeatureSplitFaceCarrierTypeAssumed,
        Self::FeatureSplitFaceIndexedOperandAssumed,
        Self::FeatureFixedFilletAssignmentNative,
        Self::FeatureFilletRadiusUnresolved,
        Self::FeatureFilletReferencesUnresolved,
        Self::FeatureFilletFullRoundCenterUnresolved,
        Self::FeatureFilletSetListNative,
        Self::FeatureExtrudeExtentNative,
        Self::FeatureExtrudeTerminationMissing,
        Self::FeatureRevolveLayoutAssumed,
        Self::FeatureRevolveFrameUnmatched,
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
        Self::FeatureLoftSolidInferred,
        Self::FeatureSurfaceExtendMethodUnresolved,
        Self::FeatureSurfaceExtendEdgesAsFaces,
        Self::FeatureSplitToolExtensionAssumed,
        Self::FeatureSplitToolExtensionUnresolved,
        Self::FeatureSplitBodyFaceToolGroupAssumed,
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
        Self::FeatureShellRemovedFacesAssumed,
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
        Self::FeatureBoundaryFillCellsNative,
        Self::FeatureBoundaryFillCellsUnresolved,
        Self::FeatureBoundaryFillOperationUnresolved,
        Self::FeatureBoundaryFillToolsUnresolved,
        Self::FeatureBoundaryFillTargetsUnresolved,
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
        Self::MeshComponentFlagUninterpreted,
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
        Self::XrefLineageDisagrees,
        Self::XrefIdentityBlank,
        Self::XrefArchiveIdentityDisagrees,
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
        Self::LocalOccurrenceMarkedPreludeAssumed,
        Self::LocalOccurrenceObjectPreludeAssumed,
        Self::PlacementChildrenDiffer,
        Self::InstanceReferenceShared,
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
        Self::EmptyDesign,
        Self::MeshOnlyDesignUntransferred,
        Self::AssemblyComponentsExternal,
        Self::NurbsSurfaceCarrier,
        Self::SketchRegionBodyOmitted,
        Self::NurbsCurveCarrier,
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
        Self::TopologyFaceBoundaryUnstated,
        Self::MissingGeometryStream,
        Self::XrefCycle,
        Self::XrefMemberMissing,
        Self::XrefMemberUndecoded,
        Self::TsplineRecordUntyped,
        Self::TsplineRadialMapUnresolved,
        Self::TsplineCageUndecoded,
        Self::SourceDialectUnverified,
        Self::WriterAppearancePropertiesOmitted,
        Self::WriterProceduralReduced,
        Self::WriterPcurveOmitted,
        Self::WriterSubdOmitted,
        Self::WriterAssetOmitted,
        Self::WriterJointOmitted,
        Self::WriterParameterOmitted,
        Self::WriterSketchContentOmitted,
        Self::WriterSketchSupportSubstituted,
        Self::WriterConfigurationOmitted,
        Self::WriterTessellationOmitted,
        Self::WriterSourceAssociationOmitted,
        Self::WriterBodyOmitted,
        Self::WriterFaceOmitted,
        Self::WriterGeometryOmitted,
        Self::WriterEdgeRangeAssumed,
        Self::WriterRollbackAssumed,
        Self::WriterAppearanceOmitted,
        Self::WriterHistoryUnrestored,
        Self::WriterAppearanceGuidAllocated,
        Self::KernelDialectUnverified,
        Self::KernelCarrierUnparseable,
        Self::DialectLayerCollision,
        Self::TextHeaderNoncanonical,
        Self::KernelHeaderToleranceUnresolved,
        Self::KernelHeaderMetadataUnresolved,
        Self::ManifestMetadataUnresolved,
        Self::UnindexedRecordsRetained,
        Self::DesignIntentUndecoded,
        Self::BodyDisplayNameAmbiguous,
        Self::WorkPlaneDisplayNameAmbiguous,
        Self::WorkPlaneDisplayNameUndecoded,
        Self::WriterIntegerAsReal,
        Self::WriterDesignRecordOmitted,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::SceneOverrideRefused => "scene.override-refused",
            Self::SettingsUpAxisUnrecognized => "settings.up-axis-unrecognized",
            Self::SettingsUpAxisRecordUnreadable => "settings.up-axis-record-unreadable",
            Self::SceneVisibilityNative => "scene.visibility-native",
            Self::DraftPartingControlsPartial => "feature.draft-parting-controls-partial",
            Self::DraftNeutralAnchorAssumed => "feature.draft-neutral-anchor-assumed",
            Self::DraftSlopeOutOfRange => "feature.draft-slope-out-of-range",
            Self::ContourVectorOwnerDistanceRoleAssumed => {
                "feature.contour-vector-owner-distance-role-assumed"
            }
            Self::BaseFaceRegionsNative => "feature.base-face-regions-native",
            Self::BaseFaceSourceCarrierUnresolved => "feature.base-face-source-carrier-unresolved",
            Self::BaseFaceThicknessUnresolved => "feature.base-face-thickness-unresolved",
            Self::BaseFaceMaterialSideUnresolved => "feature.base-face-material-side-unresolved",
            Self::BaseFaceResultChoiceUnresolved => "feature.base-face-result-choice-unresolved",
            Self::ContourProfileNative => "feature.contour-profile-native",
            Self::ContourDefaultSideOneLeftAssumed => {
                "feature.contour-default-side-one-left-assumed"
            }
            Self::ContourEnvelopeUnresolved => "feature.contour-envelope-unresolved",
            Self::ContourOwnerVectorDirectionAssumed => {
                "feature.contour-owner-vector-direction-assumed"
            }
            Self::ContourOperationFourBodyAssumed => "feature.contour-operation-four-body-assumed",
            Self::ContourResultUnresolved => "feature.contour-result-unresolved",
            Self::ContourSettingsRuleReferenceAssumed => {
                "feature.contour-settings-rule-reference-assumed"
            }
            Self::ContourRuleFirstRadiusAssumed => "feature.contour-rule-first-radius-assumed",
            Self::ContourMaterialScalarFactorAssumed => {
                "feature.contour-material-scalar-factor-assumed"
            }
            Self::ContourInlineThicknessAssumed => "feature.contour-inline-thickness-assumed",
            Self::ContourRadiusDatabaseUnitAssumed => {
                "feature.contour-radius-database-unit-assumed"
            }
            Self::ContourDistanceDatabaseUnitAssumed => {
                "feature.contour-distance-database-unit-assumed"
            }
            Self::ContourCachedOwnerDistanceAssumed => {
                "feature.contour-cached-owner-distance-assumed"
            }
            Self::ContourSingleDistanceOneSidedAssumed => {
                "feature.contour-single-distance-one-sided-assumed"
            }
            Self::ContourDistanceUnresolved => "feature.contour-distance-unresolved",
            Self::ContourFirstRadiusOverrideAssumed => {
                "feature.contour-first-radius-override-assumed"
            }
            Self::ContourDirectionUnresolved => "feature.contour-direction-unresolved",
            Self::ContourThicknessUnresolved => "feature.contour-thickness-unresolved",
            Self::ContourRadiusUnresolved => "feature.contour-radius-unresolved",
            Self::ContourRuleFactorUnresolved => "feature.contour-rule-factor-unresolved",
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
            Self::EdgeReferenceEnvelopeUnresolved => {
                "feature.edge-flange-reference-envelope-unresolved"
            }
            Self::EdgeParameterOwnerBacklinkAssumed => {
                "feature.edge-flange-parameter-owner-backlink-assumed"
            }
            Self::EdgeCachedParameterAssumed => "feature.edge-flange-cached-parameter-assumed",
            Self::EdgeParameterUnitAssumed => "feature.edge-flange-parameter-unit-assumed",
            Self::EdgeParameterUnresolved => "feature.edge-flange-parameter-unresolved",
            Self::EdgeStoredRadiusInsideAssumed => {
                "feature.edge-flange-stored-radius-inside-assumed"
            }
            Self::EdgeRadiusUnresolved => "feature.edge-flange-radius-unresolved",
            Self::EdgeSelectionNative => "feature.edge-flange-selection-native",
            Self::EdgeBoundaryReferenceNative => "feature.edge-flange-boundary-reference-native",
            Self::EdgeBoundaryScopeReferenceAssumed => {
                "feature.edge-flange-boundary-scope-reference-assumed"
            }
            Self::EdgeFullWidthAssumed => "feature.edge-flange-full-width-assumed",
            Self::EdgeSignedOffsetsAssumed => "feature.edge-flange-signed-offsets-assumed",
            Self::EdgeDatumUnresolved => "feature.edge-flange-datum-unresolved",
            Self::EdgePositionUnresolved => "feature.edge-flange-position-unresolved",
            Self::EdgeWidthUnresolved => "feature.edge-flange-width-unresolved",
            Self::EdgeHeightTargetNative => "feature.edge-flange-height-target-native",
            Self::EdgeHeightTargetScopeAssumed => "feature.edge-flange-height-target-scope-assumed",
            Self::SceneOverrideAmbiguous => "scene.override-ambiguous",
            Self::SceneOverrideUnbound => "scene.override-unbound",
            Self::ScopeOwnerUnresolved => "scope.owner-unresolved",
            Self::ScopeOutputOwnerMismatch => "scope.output-owner-mismatch",
            Self::ScopeOutputOwnerUnresolved => "scope.output-owner-unresolved",
            Self::ScopeSketchOwnerMismatch => "scope.sketch-owner-mismatch",
            Self::OperationPoseApplied => "assembly.operation-table-pose",
            Self::OperationPosePathOrdinalAssumed => {
                "assembly.operation-table-path-ordinal-assumed"
            }
            Self::OperationPoseVersionAssumed => "assembly.operation-table-version-assumed",
            Self::OperationPoseDisagrees => "assembly.operation-table-pose-disagrees",
            Self::OperationPoseNative => "assembly.operation-table-pose-native",
            Self::DimensionCompanionUntyped => "dimension.companion-untyped",
            Self::BodyReferenceTailUntyped => "body-reference.tail-untyped",
            Self::BodyRecipeSerializationFormAssumed => "body-recipe.serialization-form-assumed",
            Self::PlacementConstructionUntyped => "placement.construction-untyped",
            Self::WorkPlaneFrameUnbound => "work-plane.frame-unbound",
            Self::WorkPlaneInferredAxisAssumed => "work-plane.inferred-axis-assumed",
            Self::CylinderPlacementUnbound => "feature.cylinder-placement-unbound",
            Self::CylinderLocalFrameAssumed => "feature.cylinder-local-frame-assumed",
            Self::SpherePrefixFramingAssumed => "feature.sphere-prefix-framing-assumed",
            Self::SpherePlacementFlagsAssumed => "feature.sphere-placement-flags-assumed",
            Self::SphereLocalMatrixFramingAssumed => "feature.sphere-local-matrix-framing-assumed",
            Self::SphereLocalFrameAssumed => "feature.sphere-local-frame-assumed",
            Self::SpherePlacementFrameAssumed => "feature.sphere-placement-frame-assumed",
            Self::SphereDiameterOwnerAssumed => "feature.sphere-diameter-owner-assumed",
            Self::SphereDiameterSourceAssumed => "feature.sphere-diameter-source-assumed",

            Self::ConstructionTrackingPathUntyped => "feature.tracking-path-untyped",
            Self::ConstructionClosingFlagAssumed => "feature.construction-closing-flag-assumed",
            Self::SketchCurveTailMarkerAssumed => "sketch.curve-tail-marker-assumed",
            Self::SketchCurveTailConstructionAssumed => "sketch.curve-tail-construction-assumed",
            Self::ChamferEdgeRoleAssumed => "feature.chamfer-edge-role-assumed",
            Self::ChamferFixedFaceGroupAssumed => "feature.chamfer-fixed-face-group-assumed",
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
            Self::CoilTargetUnresolved => "feature.coil-target-unresolved",
            Self::CoilShortControlsAssumed => "feature.coil-short-controls-assumed",
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
            Self::StoredFrameOccurrence => "assembly.stored-frame-occurrence",
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
            Self::SketchContainerHeaderUnrecognized => "sketch.container-header-unrecognized",
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
            Self::OccurrenceVisibilityNative => "assembly.occurrence-visibility-native",
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
            Self::AssemblyJointOriginEnvelopeMatrixDiscarded => {
                "assembly.joint-origin-envelope-matrix-discarded"
            }
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
            Self::AssemblyMemberJointPlacementBound => "assembly.member-joint-placement-bound",
            Self::AssemblyJointSourceBindingAssumed => "assembly.joint-source-binding-assumed",
            Self::AssemblyConnectorOperandsAssumed => "assembly.connector-operands-assumed",
            Self::AssemblyMotionBindingAssumed => "assembly.motion-binding-assumed",
            Self::AssemblyAdditionalOwnerMotionAssumed => {
                "assembly.additional-owner-motion-assumed"
            }
            Self::AssemblyExtendedTailAssumed => "assembly.extended-tail-assumed",
            Self::AssemblyExtendedTypedTailAssumed => "assembly.extended-typed-tail-assumed",
            Self::AssemblyOperandNestedPathRetained => "assembly.operand-nested-path-retained",
            Self::AssemblyOperandPathWordAssumed => "assembly.operand-path-word-assumed",
            Self::DrawingDocumentOmitted => "drawing.document-omitted",
            Self::HistoryBindingBudgetExceeded => "history.binding-budget-exceeded",
            Self::HistoryBindingWorkExceeded => "history.binding-work-exceeded",
            Self::RecipeSideSearchBudgetExceeded => "feature.recipe-search-budget-exceeded",
            Self::FaceRecipeMatchWorkExceeded => "feature.face-recipe-work-exceeded",
            Self::ParameterOrderBudgetExceeded => "parameter.order-budget-exceeded",
            Self::HistoryRecordFramingFailed => "history.record-framing-failed",
            Self::DesignBodyBindingUnresolved => "design.body-binding-unresolved",
            Self::HistoryOnlyBodyOmitted => "design.history-only-body-omitted",
            Self::FlaggedBaseFeatureBodySelected => "design.flagged-base-feature-body-selected",
            Self::ReferenceImageNativeRetained => "appearance.reference-image-native",
            Self::ReferenceImageFlagAssumed => "appearance.reference-image-flag-assumed",
            Self::DecalNativeRetained => "appearance.decal-native",
            Self::DecalMappingAssumed => "appearance.decal-mapping-assumed",
            Self::DecalPlacementNative => "appearance.decal-placement-native",
            Self::DecalPlacementLayoutAssumed => "appearance.decal-placement-layout-assumed",
            Self::DecalPlacementNormalAssumed => "appearance.decal-placement-normal-assumed",
            Self::DecalTargetUnresolved => "appearance.decal-target-unresolved",
            Self::DecalImageUnresolved => "appearance.decal-image-unresolved",
            Self::DecalDefinitionUnresolved => "appearance.decal-definition-unresolved",
            Self::BodyOverridePrecedenceAssumed => "appearance.body-override-precedence-assumed",
            Self::BodyOverrideSuperseded => "appearance.body-override-superseded",
            Self::EdgeReferenceLostUnrepaired => "feature.edge-reference-lost",
            Self::FeatureStoredBodiesEmpty => "feature.stored-bodies-empty",
            Self::FeatureHistoryBodyBounded => "feature.history-body-bounded",
            Self::FeatureHistoryBodyUndecoded => "feature.history-body-undecoded",
            Self::FeatureHistoryBodyConsumedWithheld => "feature.history-body-consumed-withheld",
            Self::FeatureBodyHistoryIncomplete => "feature.body-history-incomplete",
            Self::FeatureBodyHistoryOwnerUnresolved => "feature.body-history-owner-unresolved",
            Self::FeatureBodyHistoryUnread => "feature.body-history-unread",
            Self::FeatureOwnedBodyPairingAssumed => "feature.owned-body-pairing-assumed",
            Self::FeatureStoredBodiesRecipeAbsent => "feature.stored-bodies-recipe-absent",
            Self::FeatureStoredBodiesUnbound => "feature.stored-bodies-unbound",
            Self::FeatureDerivedInstanceComponentInsertAssumed => {
                "feature.derived-instance-component-insert-assumed"
            }
            Self::ComponentInsertLegacyFrameAssumed => "assembly.legacy-insert-frame-assumed",
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
            Self::FeaturePatternFeatureSeedSubstituted => {
                "feature.pattern-feature-seed-substituted"
            }
            Self::FeatureRevolvePlaneWorkPlaneAssumed => "feature.revolve-plane-work-plane-assumed",
            Self::FeatureCircularPatternPlacementNotTransferred => {
                "feature.circular-pattern-placement-not-transferred"
            }
            Self::FeatureCircularPatternSuppressionNotTransferred => {
                "feature.circular-pattern-suppression-not-transferred"
            }
            Self::FeatureCircularPatternDistributionAssumed => {
                "feature.circular-pattern-distribution-assumed"
            }
            Self::FeatureCircularPatternControlsAssumed => {
                "feature.circular-pattern-controls-assumed"
            }
            Self::FeaturePatternPayloadSeedUnconfirmed => {
                "feature.pattern-payload-seed-unconfirmed"
            }
            Self::FeatureRectangularPatternDistanceFormAssumed => {
                "feature.rectangular-pattern-distance-form-assumed"
            }
            Self::FeatureRectangularPatternInactiveSymmetricDropped => {
                "feature.rectangular-pattern-inactive-symmetric-dropped"
            }
            Self::FeatureRectangularPatternDirectionOneRecordAssumed => {
                "feature.rectangular-pattern-direction-one-record-assumed"
            }
            Self::FeatureRectangularPatternDirectionLinksNative => {
                "feature.rectangular-pattern-direction-links-native"
            }
            Self::FeatureRectangularPatternConstructionNative => {
                "feature.rectangular-pattern-construction-native"
            }
            Self::FeatureRectangularPatternDesignNative => {
                "feature.rectangular-pattern-design-native"
            }
            Self::FeatureRectangularPatternDirectionUnresolved => {
                "feature.rectangular-pattern-direction-unresolved"
            }
            Self::FeatureRectangularPatternDirectionRecordDisagrees => {
                "feature.rectangular-pattern-direction-record-disagrees"
            }
            Self::FeatureOutputStateChainUnbound => "feature.output-state-chain-unbound",
            Self::FeatureOutputBodyUnbound => "feature.output-body-unbound",
            Self::FeaturePatternCopyBodyOmitted => "feature.pattern-copy-body-omitted",
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
            Self::FeatureMeshTessellateScopeUnparsed => "feature.mesh-tessellate-scope-unparsed",
            Self::FeatureMeshTessellateFieldsUninterpreted => {
                "feature.mesh-tessellate-fields-uninterpreted"
            }
            Self::FeatureMeshTessellateSourceUnresolved => {
                "feature.mesh-tessellate-source-unresolved"
            }
            Self::FeatureMeshPlaneCutScopeUnparsed => "feature.mesh-plane-cut-scope-unparsed",
            Self::FeatureMeshPlaneCutTailPartial => "feature.mesh-plane-cut-tail-partial",
            Self::FeatureMeshPlaneCutPlanePartial => "feature.mesh-plane-cut-plane-partial",
            Self::FeatureMeshPlaneCutSelectorUnruled => "feature.mesh-plane-cut-selector-unruled",
            Self::FeatureMeshPlaneCutFieldsUninterpreted => {
                "feature.mesh-plane-cut-fields-uninterpreted"
            }
            Self::FeatureMeshPlaneCutSourceUnresolved => "feature.mesh-plane-cut-source-unresolved",
            Self::FeatureMeshPlaneCutSelectionUnresolved => {
                "feature.mesh-plane-cut-selection-unresolved"
            }
            Self::FeatureMeshPlaneCutMembershipAssumed => {
                "feature.mesh-plane-cut-membership-assumed"
            }
            Self::FeatureMeshPlaneCutMatrixOrientationAssumed => {
                "feature.mesh-plane-cut-matrix-orientation-assumed"
            }
            Self::FeatureMeshPlaneCutUnitsAssumed => "feature.mesh-plane-cut-units-assumed",
            Self::FeatureMeshPlaneCutModelSpaceAssumed => {
                "feature.mesh-plane-cut-model-space-assumed"
            }
            Self::FeatureMeshPlaneCutRetainedSideAssumed => {
                "feature.mesh-plane-cut-retained-side-assumed"
            }
            Self::FeatureMeshPlaneCutSplitFacesAssumed => {
                "feature.mesh-plane-cut-split-faces-assumed"
            }
            Self::FeatureMeshPlaneCutUniformFillAssumed => {
                "feature.mesh-plane-cut-uniform-fill-assumed"
            }
            Self::FeatureMeshCombineScopeUnparsed => "feature.mesh-combine-scope-unparsed",
            Self::FeatureMeshCombineTailPartial => "feature.mesh-combine-tail-partial",
            Self::FeatureMeshCombineSelectorUnruled => "feature.mesh-combine-selector-unruled",
            Self::FeatureMeshCombineFieldsUninterpreted => {
                "feature.mesh-combine-fields-uninterpreted"
            }
            Self::FeatureMeshCombineSourceUnresolved => "feature.mesh-combine-source-unresolved",
            Self::FeatureMeshCombineSelectionUnresolved => {
                "feature.mesh-combine-selection-unresolved"
            }
            Self::FeatureMeshCombineMergeAssumed => "feature.mesh-combine-merge-assumed",
            Self::FeatureMeshCombineLegacyBooleanAssumed => {
                "feature.mesh-combine-legacy-boolean-assumed"
            }
            Self::FeatureMeshCombineKeepToolsAssumedFalse => {
                "feature.mesh-combine-keep-tools-assumed-false"
            }
            Self::FeatureMeshCombineDestinationAssumedTargetComponent => {
                "feature.mesh-combine-destination-assumed-target-component"
            }
            Self::FeatureMeshCombineKeepToolsUnresolved => {
                "feature.mesh-combine-keep-tools-unresolved"
            }
            Self::FeatureMeshCombineDestinationUnresolved => {
                "feature.mesh-combine-destination-unresolved"
            }
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
            Self::ThreadModeledFlagAssumed => "feature.thread-modeled-flag-assumed",
            Self::ThreadModeledEntriesAbsent => "feature.thread-modeled-entries-absent",
            Self::ThreadConstructionUnresolved => "feature.thread-construction-unresolved",
            Self::ThreadOffsetNative => "feature.thread-offset-native",
            Self::ThreadAngleNative => "feature.thread-angle-native",
            Self::ThreadDetailNative => "feature.thread-detail-native",
            Self::ThreadHoleNative => "feature.thread-hole-native",
            Self::ThreadFractionalSizeDiameterAssumed => {
                "feature.thread-fractional-size-diameter-assumed"
            }
            Self::FeatureMeshSeparateScopeUnparsed => "feature.mesh-separate-scope-unparsed",
            Self::FeatureMeshSeparateTailPartial => "feature.mesh-separate-tail-partial",
            Self::FeatureMeshSeparateReferenceUnresolved => {
                "feature.mesh-separate-reference-unresolved"
            }
            Self::FeatureMeshSeparateReferenceMismatch => {
                "feature.mesh-separate-reference-mismatch"
            }
            Self::FeatureMeshSeparateSelectorUnruled => "feature.mesh-separate-selector-unruled",
            Self::FeatureMeshSeparateShellsMethodAssumed => {
                "feature.mesh-separate-shells-method-assumed"
            }
            Self::FeatureMeshSeparateFaceGroupsMethodAssumed => {
                "feature.mesh-separate-face-groups-method-assumed"
            }
            Self::FeatureMeshSeparateAllGroupsAssumed => "feature.mesh-separate-all-groups-assumed",
            Self::FeatureMeshSeparateSelectedGroupsAssumed => {
                "feature.mesh-separate-selected-groups-assumed"
            }
            Self::FeatureMeshSeparateMultipleBodiesCarrierAssumed => {
                "feature.mesh-separate-multiple-bodies-carrier-assumed"
            }
            Self::FeatureMeshSeparateMultipleBodiesPolarityAssumed => {
                "feature.mesh-separate-multiple-bodies-polarity-assumed"
            }
            Self::FeatureMeshSeparateKeepBodyCarrierAssumed => {
                "feature.mesh-separate-keep-body-carrier-assumed"
            }
            Self::FeatureMeshSeparateKeepBodyPolarityAssumed => {
                "feature.mesh-separate-keep-body-polarity-assumed"
            }
            Self::FeatureMeshSeparatePrefixAssumed => "feature.mesh-separate-prefix-assumed",
            Self::FeatureMeshSeparateOutputRoleAssumed => {
                "feature.mesh-separate-output-role-assumed"
            }
            Self::FeatureMeshSeparateOutputUnresolved => "feature.mesh-separate-output-unresolved",
            Self::FeatureMeshSeparateSourceMembershipAssumed => {
                "feature.mesh-separate-source-membership-assumed"
            }
            Self::FeatureMeshSeparateSourceUnresolved => "feature.mesh-separate-source-unresolved",
            Self::FeatureMeshSeparateSourceFieldsUninterpreted => {
                "feature.mesh-separate-source-fields-uninterpreted"
            }
            Self::FeatureMeshSeparateReferenceRolesAssumed => {
                "feature.mesh-separate-reference-roles-assumed"
            }
            Self::FeatureMeshSeparateMembershipUnresolved => {
                "feature.mesh-separate-membership-unresolved"
            }
            Self::SheetThicknessUnruled => "feature.sheet-thickness-unruled",
            Self::SheetMetalRuleControlsNative => "feature.sheet-metal-rule-controls-native",
            Self::SheetMetalProfileNative => "feature.sheet-metal-profile-native",
            Self::BaseFlangeProfileUnresolved => "feature.base-flange-profile-unresolved",
            Self::BaseFlangeThicknessUnresolved => "feature.base-flange-thickness-unresolved",
            Self::BaseFlangeSideUnresolved => "feature.base-flange-side-unresolved",
            Self::BaseFlangeSideAssumed => "feature.base-flange-side-assumed",
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
            Self::HolePlacementPointUnselected => "feature.hole-placement-point-unselected",
            Self::FeaturePointDirectionInputsUnrepresented => {
                "feature.point-direction-inputs-unrepresented"
            }
            Self::HoleReferenceDimensionsNative => "feature.hole-reference-dimensions-native",
            Self::HoleParametersIndependentAssumed => "feature.hole-parameters-independent-assumed",
            Self::HoleShapePartialAssumed => "feature.hole-shape-partial-assumed",
            Self::HoleExtentLeadingWordAssumed => "feature.hole-extent-leading-word-assumed",
            Self::HoleThreadMinorDiameterAssumed => "feature.hole-thread-minor-diameter-assumed",
            Self::HoleTappedDiameterStored => "feature.hole-tapped-diameter-stored",
            Self::HoleTaperTappedBoreCylindrical => "feature.hole-taper-tapped-bore-cylindrical",
            Self::FeatureBaseFeatureNullFlagAssumed => "feature.base-feature-null-flag-assumed",
            Self::FeatureBaseFeatureSuffixRunOrderAssumed => {
                "feature.base-feature-suffix-run-order-assumed"
            }
            Self::FeatureFilletPaddingAssumed => "feature.fillet-padding-assumed",
            Self::FeatureFilletMidpointOrderAssumed => "feature.fillet-midpoint-order-assumed",
            Self::FeatureFilletSetInputUnresolved => "feature.fillet-set-input-unresolved",
            Self::FeatureFilletSetGroupUnresolved => "feature.fillet-set-group-unresolved",
            Self::HoleStandardSizingAssumed => "feature.hole-standard-sizing-assumed",
            Self::HoleClearanceFieldsAssumed => "feature.hole-clearance-fields-assumed",
            Self::HoleCarrierAxisAssumed => "feature.hole-carrier-axis-assumed",
            Self::FeatureScopeUnprojected => "feature.scope-unprojected",
            Self::MoveUnprojected => "feature.move-unprojected",
            Self::ModifyUnprojected => "feature.modify-unprojected",
            Self::ParameterUnprojected => "parameter.unprojected",
            Self::ParameterRecordUnparsed => "parameter.record-unparsed",
            Self::ParameterTrailerFlagUninterpreted => "parameter.trailer-flag-uninterpreted",
            Self::ParameterHeaderFlagUninterpreted => "parameter.header-flag-uninterpreted",
            Self::ParameterDrivenFlagAssumed => "parameter.driven-flag-assumed",
            Self::ParameterOwnerTrailerFlagUninterpreted => {
                "parameter.owner-trailer-flag-uninterpreted"
            }
            Self::ParameterOwnerUnstored => "parameter.owner-unstored",
            Self::ParameterOwnerUnrecognized => "parameter.owner-unrecognized",
            Self::ParameterOwnerUnparsed => "parameter.owner-unparsed",
            Self::ParameterUnitUntyped => "parameter.unit-untyped",
            Self::MaterialDistanceUnitUntyped => "material.distance-unit-untyped",
            Self::FaceAppearanceAmbiguous => "material.face-appearance-ambiguous",
            Self::ParameterExpressionUnbound => "parameter.expression-unbound",
            Self::ParameterReferencedDesignUnscoped => "parameter.referenced-design-unscoped",
            Self::ParameterOwnerOutsideTimeline => "parameter.owner-outside-timeline",
            Self::ParameterOwnerUnresolvedOmitted => "parameter.owner-unresolved-omitted",
            Self::ParameterNameBlank => "parameter.name-blank",
            Self::ConfigurationReferencedDesignUnscoped => {
                "configuration.referenced-design-unscoped"
            }
            Self::ConfigurationOverrideUnstated => "configuration.override-unstated",
            Self::ConfigurationOverrideValueUnprojected => {
                "configuration.override-value-unprojected"
            }
            Self::ConfigurationScopeWithoutActiveRow => "configuration.scope-without-active-row",
            Self::ConfigurationPropertyUnprojected => "configuration.property-unprojected",
            Self::HistoryRollbackUnread => "history.rollback-unread",
            Self::HistoryRollbackBoundUnread => "history.rollback-bound-unread",
            Self::CoordinateSystemConstructionNative => {
                "feature.coordinate-system-construction-native"
            }
            Self::CoordinateSystemConstructionDropped => {
                "feature.coordinate-system-construction-dropped"
            }
            Self::CoordinateSystemFrameUnresolved => "feature.coordinate-system-frame-unresolved",
            Self::HistoryDependencyUnprojected => "history.dependency-unprojected",
            Self::HistoricalFaceSelectionUnbound => "history.face-selection-unbound",
            Self::HistoryDeleteFaceMemberOrderAssumed => "history.delete-face-member-order-assumed",
            Self::HistoricalEdgeMidpointsUnresolved => "history.edge-midpoints-unresolved",
            Self::HistoricalFilletEdgeInferred => "history.fillet-edge-inferred",
            Self::HistoricalEdgeSelectionInferred => "history.edge-selection-inferred",
            Self::HistoricalEdgeSelectionDeletedSetAssumed => {
                "history.edge-selection-deleted-set-assumed"
            }
            Self::HistoricalEdgeSelectionLeftoverAssumed => {
                "history.edge-selection-leftover-assumed"
            }
            Self::HistoricalEdgeSelectionFirstSidePairAssumed => {
                "history.edge-selection-first-side-pair-assumed"
            }
            Self::HistoricalEdgeContinuityAssumedSharp => "history.edge-continuity-assumed-sharp",
            Self::HistoricalEdgeHistoryInferred => "history.edge-history-inferred",
            Self::HistoricalOutputBlobHistoryInferred => "history.output-blob-history-inferred",
            Self::HistoricalEdgeRecipeHistoryInferred => "history.edge-recipe-history-inferred",
            Self::HistoricalBodyRecipeHistoryInferred => "history.body-recipe-history-inferred",
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
            Self::SketchCurveUndecoded => "sketch.curve-undecoded",
            Self::SketchCurveRecordUndecoded => "sketch.curve-record-undecoded",
            Self::SketchSurfaceUndecoded => "sketch.surface-undecoded",
            Self::SketchTextUndecoded => "sketch.text-undecoded",
            Self::SketchRelationUndecoded => "sketch.relation-undecoded",
            Self::SketchFolderUndecoded => "sketch.folder-undecoded",
            Self::SketchOrdinalCollectionUndecoded => "sketch.ordinal-collection-undecoded",
            Self::SketchContainerUndecoded => "sketch.container-undecoded",
            Self::SketchGeometryGroupUndecoded => "sketch.geometry-group-undecoded",
            Self::FeatureTimelineUndecoded => "feature.timeline-undecoded",
            Self::PrimaryRecordUndecoded => "design.primary-record-undecoded",
            Self::ConfigurationProjectionNative => "configuration.projection-native",
            Self::SketchOwnerConflict => "sketch.owner-conflict",
            Self::SketchRelationOwnerUnresolved => "sketch.relation-owner-unresolved",
            Self::ConfigurationDocumentUndecoded => "configuration.document-undecoded",
            Self::BendSettingsUndecoded => "feature.bend-settings-undecoded",
            Self::DesignBodyBoundsUndecoded => "design.body-bounds-undecoded",
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
            Self::FeatureUnevaluatedStateAssumed => "feature.unevaluated-state-assumed",
            Self::FeatureNoResultActiveAssumed => "feature.no-result-active-assumed",
            Self::FeatureStateByteAboveOne => "feature.state-byte-above-one",
            Self::FeatureExtrudeProfileGroupNative => "feature.extrude-profile-group-native",
            Self::FeatureExtrudeSketchTextSkipped => "feature.extrude-sketch-text-skipped",
            Self::FeatureExtrudeProfileGroupUnionAssumed => {
                "feature.extrude-profile-group-union-assumed"
            }
            Self::FeatureExtrudeProfileSketchAbsent => "feature.extrude-profile-sketch-absent",
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
            Self::FeatureExtrudeOpaqueDistanceWordAssumed => {
                "feature.extrude-opaque-distance-word-assumed"
            }
            Self::FeatureExtrudeStartFaceAbsentAssumed => {
                "feature.extrude-start-face-absent-assumed"
            }
            Self::FeatureExtrudeStartFaceUnresolved => "feature.extrude-start-face-unresolved",
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
            Self::FeatureExtrudeProfileFaceCurved => "feature.extrude-profile-face-curved",
            Self::FeatureProfileSelectionNative => "feature.profile-selection-native",
            Self::FeatureProfileArrangementCapped => "feature.profile-arrangement-capped",
            Self::FeatureProfileJoinEnvelopeCapped => "feature.profile-join-envelope-capped",
            Self::FeatureProfileCrossingRunsCapped => "feature.profile-crossing-runs-capped",
            Self::FeatureProfileDensificationCapped => "feature.profile-densification-capped",
            Self::FeatureProfileCrossingLoopCapped => "feature.profile-crossing-loop-capped",
            Self::FeatureProfileCrossingLoopAssumed => "feature.profile-crossing-loop-assumed",
            Self::FeatureProfileLoopFlagsAssumed => "feature.profile-loop-flags-assumed",
            Self::FeatureProfileCoincidentDirectionAssumed => {
                "feature.profile-coincident-direction-assumed"
            }
            Self::FeatureProfileCoincidentBoundaryAssumed => {
                "feature.profile-coincident-boundary-assumed"
            }
            Self::FeatureProfileCrossingRunsAssumed => "feature.profile-crossing-runs-assumed",
            Self::FeatureProfilePartialEntryAssumed => "feature.profile-partial-entry-assumed",
            Self::FeatureProfileRegionIdentitiesStale => "feature.profile-region-identities-stale",
            Self::FeatureProfileMemberReferenceLost => "feature.profile-member-reference-lost",
            Self::FeatureProfileTextGlyphsAssumed => "feature.profile-text-glyphs-assumed",
            Self::FeatureProfileTextGlyphsNative => "feature.profile-text-glyphs-native",
            Self::FeatureProfileCutRegionsAssumed => "feature.profile-cut-regions-assumed",
            Self::FeatureProfileIslandAbsenceAssumed => "feature.profile-island-absence-assumed",
            Self::FeatureProfilePiecePartitionAssumed => "feature.profile-piece-partition-assumed",
            Self::FeatureProfilePolylineSideAssumed => "feature.profile-polyline-side-assumed",
            Self::FeatureProfileCapOutlineAssumed => "feature.profile-cap-outline-assumed",
            Self::FeatureProfileCapCarrierAssumed => "feature.profile-cap-carrier-assumed",
            Self::FeatureProfileHoleBoundaryInferred => "feature.profile-hole-boundary-inferred",
            Self::FeatureProfileRegionHoleInferred => "feature.profile-region-hole-inferred",
            Self::FeatureProfileBodyCircleRingsAssumed => {
                "feature.profile-body-circle-rings-assumed"
            }
            Self::FeatureSpatialProfileTransitionAssumed => {
                "feature.spatial-profile-transition-assumed"
            }
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
            Self::FeatureFormSourceUnavailable => "feature.form-source-unavailable",
            Self::FeatureFormDefinitionUnavailable => "feature.form-definition-unavailable",
            Self::FeatureFormMembershipUnresolved => "feature.form-membership-unresolved",
            Self::FeatureFormLegacySerializerMembershipAssumed => {
                "feature.form-legacy-serializer-membership-assumed"
            }
            Self::FeatureFormLegacySerializerOrderAssumed => {
                "feature.form-legacy-serializer-order-assumed"
            }
            Self::FeatureFormCageBindingAmbiguous => "feature.form-cage-binding-ambiguous",
            Self::FeatureFormBodyReferencesPartialAssumed => {
                "feature.form-body-references-partial-assumed"
            }
            Self::FeatureFormBodyRegistrationUnsupported => {
                "feature.form-body-registration-unsupported"
            }
            Self::FeatureFormReferenceTypeConflict => "feature.form-reference-type-conflict",
            Self::FeatureFormForeignReferenceUnresolved => {
                "feature.form-foreign-reference-unresolved"
            }
            Self::FeatureFormTrackingOwnerConflict => "feature.form-tracking-owner-conflict",
            Self::FeatureFormTrackingRunUnresolved => "feature.form-tracking-run-unresolved",
            Self::FeatureFormTrackingFixedSlotsAssumed => {
                "feature.form-tracking-fixed-slots-assumed"
            }
            Self::FeatureFormSerializerReferenceUnresolved => {
                "feature.form-serializer-reference-unresolved"
            }
            Self::FeatureFormMembershipConflict => "feature.form-membership-conflict",
            Self::FeatureScaleFrameLengthFormAssumed => "feature.scale-frame-length-form-assumed",
            Self::FeatureScaleMatrixWordAssumed => "feature.scale-matrix-word-assumed",
            Self::FeatureScaleUnlistedGroupsAssumed => "feature.scale-unlisted-groups-assumed",
            Self::FeatureWorkAxisRuleUnassigned => "feature.work-axis-rule-unassigned",
            Self::FeatureWorkAxisNonzeroParametersSolvedAssumed => {
                "feature.work-axis-nonzero-parameters-solved-assumed"
            }
            Self::FeatureWorkAxisPartialCarrierSolvedAssumed => {
                "feature.work-axis-partial-carrier-solved-assumed"
            }
            Self::FeatureWorkAxisTwoPointOperandsUnresolvedSolvedAssumed => {
                "feature.work-axis-two-point-operands-unresolved-solved-assumed"
            }
            Self::FeatureWorkAxisTwoPointConflictSolvedAssumed => {
                "feature.work-axis-two-point-conflict-solved-assumed"
            }
            Self::FeatureWorkAxisGeometryUnrepresentable => {
                "feature.work-axis-geometry-unrepresentable"
            }
            Self::FeatureWorkAxisBindingUnresolved => "feature.work-axis-binding-unresolved",
            Self::FeatureWorkAxisLeadingBlockLayoutAssumed => {
                "feature.work-axis-leading-block-layout-assumed"
            }
            Self::FeatureWorkAxisUnsupportedVersionLegacyLayoutAssumed => {
                "feature.work-axis-unsupported-version-legacy-layout-assumed"
            }
            Self::FeatureWorkAxisLegacyLayoutAssumed => "feature.work-axis-legacy-layout-assumed",
            Self::FeatureWorkAxisCandidatesUnresolved => "feature.work-axis-candidates-unresolved",
            Self::FeatureWorkAxisInputNull => "feature.work-axis-input-null",
            Self::FeatureWorkAxisPartialLineNative => "feature.work-axis-partial-line-native",
            Self::FeatureWorkAxisEndpointToleranceAssumed => {
                "feature.work-axis-endpoint-tolerance-assumed"
            }
            Self::FeatureBodySelectionNative => "feature.body-selection-native",
            Self::FeatureGeneratedBodyHistoryUnresolved => {
                "feature.generated-body-history-unresolved"
            }
            Self::FeatureReverseNormalBodyUnverified => "feature.reverse-normal-body-unverified",
            Self::FeatureReverseNormalBodyProducerUnresolved => {
                "feature.reverse-normal-body-producer-unresolved"
            }
            Self::FeatureReverseNormalLinkUnplaced => "feature.reverse-normal-link-unplaced",
            Self::FeatureReverseNormalLinkShared => "feature.reverse-normal-link-shared",
            Self::FeatureReverseNormalLinkOrderAssumed => {
                "feature.reverse-normal-link-order-assumed"
            }
            Self::FeatureCombineBodyProducerUnresolved => {
                "feature.combine-body-producer-unresolved"
            }
            Self::FeatureCombineOperandWriterAssumed => "feature.combine-operand-writer-assumed",
            Self::FeatureCombineToolExternalFinalStateAssumed => {
                "feature.combine-tool-external-final-state-assumed"
            }
            Self::FeatureSilhouetteSplitPrefixUnparsed => {
                "feature.silhouette-split-prefix-unparsed"
            }
            Self::FeatureSilhouetteSplitModeUnresolved => {
                "feature.silhouette-split-mode-unresolved"
            }
            Self::FeatureSilhouetteSplitDirectionUnresolved => {
                "feature.silhouette-split-direction-unresolved"
            }
            Self::FeatureSilhouetteSplitTargetUnresolved => {
                "feature.silhouette-split-target-unresolved"
            }
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
            Self::ReplaceFaceFrameAssumed => "feature.replace-face-frame-assumed",
            Self::ReplaceFaceGroupsAggregatedAssumed => {
                "feature.replace-face-groups-aggregated-assumed"
            }
            Self::ReplaceFaceOverlapUnresolved => "feature.replace-face-overlap-unresolved",
            Self::ReplaceFaceSelectionAssumed => "feature.replace-face-selection-assumed",
            Self::ReplaceFaceTangentChainAssumed => "feature.replace-face-tangent-chain-assumed",
            Self::ReplaceFaceSupportAssumed => "feature.replace-face-support-assumed",
            Self::ReplaceFaceIdentityAssumed => "feature.replace-face-identity-assumed",
            Self::ReplaceFaceFlaggedPrefixWordAssumed => {
                "feature.replace-face-flagged-prefix-word-assumed"
            }
            Self::SurfaceUntrimFrameAssumed => "feature.surface-untrim-frame-assumed",
            Self::SurfaceUntrimGroupSelectionAssumed => {
                "feature.surface-untrim-group-selection-assumed"
            }
            Self::SurfaceUntrimStoredLoopGroupsAssumed => {
                "feature.surface-untrim-stored-loop-groups-assumed"
            }
            Self::SurfaceUntrimLoopGroupsAggregatedAssumed => {
                "feature.surface-untrim-loop-groups-aggregated-assumed"
            }
            Self::SurfaceUntrimExtensionUnresolved => "feature.surface-untrim-extension-unresolved",
            Self::SurfaceUntrimExtensionUnrepresentable => {
                "feature.surface-untrim-extension-unrepresentable"
            }
            Self::SurfaceUntrimNegativeExtensionAssumed => {
                "feature.surface-untrim-negative-extension-assumed"
            }
            Self::SurfaceUntrimPaddingAssumed => "feature.surface-untrim-padding-assumed",
            Self::SurfaceUntrimTrailingIdentityAssumed => {
                "feature.surface-untrim-trailing-identity-assumed"
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
            Self::FeatureCombineToolBodyUnbound => "feature.combine-tool-body-unbound",
            Self::FeatureCombineToolExternalBodyUnbound => {
                "feature.combine-tool-external-body-unbound"
            }
            Self::FeatureCombineToolResultStateUnbound => {
                "feature.combine-tool-result-state-unbound"
            }
            Self::FeatureCombineOperandBodyUnbound => "feature.combine-operand-body-unbound",
            Self::FeatureCombineToolRecipeReplacementAssumed => {
                "feature.combine-tool-recipe-replacement-assumed"
            }
            Self::FeatureCombinePrologueAssumed => "feature.combine-prologue-assumed",
            Self::FeatureCombineOperationEntriesAssumed => {
                "feature.combine-operation-entries-assumed"
            }
            Self::FeatureCombineKeptToolsCutAssumed => "feature.combine-kept-tools-cut-assumed",
            Self::FeatureCombineOperandListRefused => "feature.combine-operand-list-refused",
            Self::FeatureCombineToolConsumptionAssumed => {
                "feature.combine-tool-consumption-assumed"
            }
            Self::FeatureSplitFaceMethodUnresolved => "feature.split-face-method-unresolved",
            Self::FeatureSplitFaceDirectionUnresolved => "feature.split-face-direction-unresolved",
            Self::FeatureSplitFaceGroupsUnresolved => "feature.split-face-groups-unresolved",
            Self::FeatureSplitFaceSelectionUnresolved => "feature.split-face-selection-unresolved",
            Self::FeatureSplitFaceSelectionProjectionFailed => {
                "feature.split-face-selection-projection-failed"
            }
            Self::FeatureSplitFaceCarrierTypeAssumed => "feature.split-face-carrier-type-assumed",
            Self::FeatureSplitFaceIndexedOperandAssumed => {
                "feature.split-face-indexed-operand-assumed"
            }
            Self::FeatureFixedFilletAssignmentNative => "feature.fixed-fillet-assignment-native",
            Self::FeatureFilletRadiusUnresolved => "feature.fillet-radius-unresolved",
            Self::FeatureFilletReferencesUnresolved => "feature.fillet-references-unresolved",
            Self::FeatureFilletFullRoundCenterUnresolved => {
                "feature.fillet-full-round-center-unresolved"
            }
            Self::FeatureFilletSetListNative => "feature.fillet-set-list-native",
            Self::FeatureExtrudeExtentNative => "feature.extrude-extent-native",
            Self::FeatureExtrudeTerminationMissing => "feature.extrude-termination-missing",
            Self::FeatureRevolveLayoutAssumed => "feature.revolve-layout-assumed",
            Self::FeatureRevolveFrameUnmatched => "feature.revolve-frame-unmatched",
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
            Self::FeatureLoftSolidInferred => "feature.loft-solid-inferred",
            Self::FeatureSplitBodyFaceToolGroupAssumed => {
                "feature.split-body-face-tool-group-assumed"
            }
            Self::FeatureSplitToolExtensionAssumed => "feature.split-tool-extension-assumed",
            Self::FeatureSplitToolExtensionUnresolved => "feature.split-tool-extension-unresolved",
            Self::FeatureSurfaceExtendMethodUnresolved => {
                "feature.surface-extend-method-unresolved"
            }
            Self::FeatureSurfaceExtendEdgesAsFaces => "feature.surface-extend-edges-as-faces",
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
            Self::FeatureShellRemovedFacesAssumed => "feature.shell-removed-faces-assumed",
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
            Self::FeatureBoundaryFillCellsNative => "feature.boundary-fill-cells-native",
            Self::FeatureBoundaryFillCellsUnresolved => "feature.boundary-fill-cells-unresolved",
            Self::FeatureBoundaryFillOperationUnresolved => {
                "feature.boundary-fill-operation-unresolved"
            }
            Self::FeatureBoundaryFillToolsUnresolved => "feature.boundary-fill-tools-unresolved",
            Self::FeatureBoundaryFillTargetsUnresolved => {
                "feature.boundary-fill-targets-unresolved"
            }
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
            Self::MeshComponentFlagUninterpreted => "mesh.component-flag-uninterpreted",
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
            Self::XrefLineageDisagrees => "xref.lineage-disagrees",
            Self::XrefIdentityBlank => "xref.identity-blank",
            Self::XrefArchiveIdentityDisagrees => "xref.archive-identity-disagrees",

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
            Self::LocalOccurrenceMarkedPreludeAssumed => {
                "assembly.local-occurrence-marked-prelude-assumed"
            }
            Self::LocalOccurrenceObjectPreludeAssumed => {
                "assembly.local-occurrence-object-prelude-assumed"
            }
            Self::PlacementChildrenDiffer => "assembly.placement-children-differ",
            Self::InstanceReferenceShared => "assembly.instance-reference-shared",
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
            Self::EmptyDesign => "design.empty",
            Self::MeshOnlyDesignUntransferred => "design.mesh-only-untransferred",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::NurbsSurfaceCarrier => "geometry.nurbs-surface-carrier",
            Self::SketchRegionBodyOmitted => "geometry.sketch-region-body-omitted",
            Self::NurbsCurveCarrier => "geometry.nurbs-curve-carrier",
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
            Self::TopologyFaceBoundaryUnstated => "topology.face-boundary-unstated",
            Self::MissingGeometryStream => "container.missing-geometry-stream",
            Self::XrefCycle => "xref.cycle",
            Self::XrefMemberMissing => "xref.member-missing",
            Self::XrefMemberUndecoded => "xref.member-undecoded",
            Self::TsplineRecordUntyped => "tspline.record-untyped",
            Self::TsplineRadialMapUnresolved => "tspline.radial-map-unresolved",
            Self::TsplineCageUndecoded => "tspline.cage-undecoded",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::WriterAppearancePropertiesOmitted => "writer.appearance-properties-omitted",
            Self::WriterProceduralReduced => "writer.procedural-reduced",
            Self::WriterPcurveOmitted => "writer.pcurve-omitted",
            Self::WriterSubdOmitted => "writer.subd-omitted",
            Self::WriterAssetOmitted => "writer.asset-omitted",
            Self::WriterJointOmitted => "writer.joint-omitted",
            Self::WriterParameterOmitted => "writer.parameter-omitted",
            Self::WriterSketchContentOmitted => "writer.sketch-content-omitted",
            Self::WriterSketchSupportSubstituted => "writer.sketch-support-substituted",
            Self::WriterConfigurationOmitted => "writer.configuration-omitted",
            Self::WriterTessellationOmitted => "writer.tessellation-omitted",
            Self::WriterSourceAssociationOmitted => "writer.source-association-omitted",
            Self::WriterBodyOmitted => "writer.body-omitted",
            Self::WriterFaceOmitted => "writer.face-omitted",
            Self::WriterGeometryOmitted => "writer.geometry-omitted",
            Self::WriterEdgeRangeAssumed => "writer.edge-range-assumed",
            Self::WriterRollbackAssumed => "writer.rollback-assumed",
            Self::WriterAppearanceOmitted => "writer.appearance-omitted",
            Self::WriterHistoryUnrestored => "writer.history-unrestored",
            Self::WriterIntegerAsReal => "writer.integer-as-real",
            Self::WriterDesignRecordOmitted => "writer.design-record-omitted",
            Self::WriterAppearanceGuidAllocated => "writer.appearance-guid-allocated",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::KernelCarrierUnparseable => "source.kernel-carrier-unparseable",
            Self::DialectLayerCollision => "source.dialect-layer-collision",
            Self::TextHeaderNoncanonical => "brep.text-header-noncanonical",
            Self::KernelHeaderToleranceUnresolved => "brep.header-tolerance-unresolved",
            Self::KernelHeaderMetadataUnresolved => "brep.header-metadata-unresolved",
            Self::ManifestMetadataUnresolved => "source.manifest-metadata-unresolved",
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
            Self::FeatureBodyHistoryIncomplete
            | Self::FeatureBodyHistoryOwnerUnresolved
            | Self::FeatureBodyHistoryUnread => Severity::Warning,
            Self::SceneOverrideRefused
            | Self::SettingsUpAxisUnrecognized
            | Self::SettingsUpAxisRecordUnreadable => Severity::Warning,
            Self::SceneOverrideRefused | Self::SceneVisibilityNative => Severity::Warning,
            Self::BendStationaryRoleAssumed
            | Self::BendAngleUnresolved
            | Self::BendRadiusFirstAssumed
            | Self::BendLineOwnerOrderAssumed
            | Self::BendStationarySelectionNative
            | Self::BendRuleRadiusUnresolved
            | Self::BendAngleOwnerBacklinkAssumed
            | Self::BendCachedOwnerAngleAssumed
            | Self::EdgeReferenceEnvelopeUnresolved
            | Self::EdgeParameterOwnerBacklinkAssumed
            | Self::EdgeCachedParameterAssumed
            | Self::EdgeParameterUnitAssumed
            | Self::EdgeParameterUnresolved
            | Self::EdgeStoredRadiusInsideAssumed
            | Self::EdgeRadiusUnresolved
            | Self::EdgeSelectionNative
            | Self::EdgeBoundaryReferenceNative
            | Self::EdgeBoundaryScopeReferenceAssumed
            | Self::EdgeFullWidthAssumed
            | Self::EdgeSignedOffsetsAssumed
            | Self::EdgeDatumUnresolved
            | Self::EdgePositionUnresolved
            | Self::EdgeWidthUnresolved
            | Self::EdgeHeightTargetNative
            | Self::EdgeHeightTargetScopeAssumed
            | Self::DraftPartingControlsPartial
            | Self::DraftNeutralAnchorAssumed
            | Self::DraftSlopeOutOfRange
            | Self::BaseFaceRegionsNative
            | Self::BaseFaceSourceCarrierUnresolved
            | Self::BaseFaceThicknessUnresolved
            | Self::BaseFaceMaterialSideUnresolved
            | Self::BaseFaceResultChoiceUnresolved
            | Self::ContourVectorOwnerDistanceRoleAssumed
            | Self::ContourProfileNative
            | Self::ContourDefaultSideOneLeftAssumed
            | Self::ContourEnvelopeUnresolved
            | Self::ContourOwnerVectorDirectionAssumed
            | Self::ContourOperationFourBodyAssumed
            | Self::ContourResultUnresolved
            | Self::ContourSettingsRuleReferenceAssumed
            | Self::ContourRuleFirstRadiusAssumed
            | Self::ContourMaterialScalarFactorAssumed
            | Self::ContourInlineThicknessAssumed
            | Self::ContourRadiusDatabaseUnitAssumed
            | Self::ContourDistanceDatabaseUnitAssumed
            | Self::ContourCachedOwnerDistanceAssumed
            | Self::ContourSingleDistanceOneSidedAssumed
            | Self::ContourDistanceUnresolved
            | Self::ContourFirstRadiusOverrideAssumed
            | Self::ContourDirectionUnresolved
            | Self::ContourThicknessUnresolved
            | Self::ContourRadiusUnresolved
            | Self::ContourRuleFactorUnresolved
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
            Self::WriterProceduralReduced => Severity::Warning,
            Self::WriterPcurveOmitted => Severity::Info,
            Self::WriterSubdOmitted => Severity::Warning,
            Self::WriterAssetOmitted => Severity::Warning,
            Self::WriterJointOmitted => Severity::Warning,
            Self::WriterParameterOmitted => Severity::Warning,
            Self::WriterSketchContentOmitted => Severity::Warning,
            Self::WriterSketchSupportSubstituted => Severity::Warning,
            Self::WriterConfigurationOmitted => Severity::Warning,
            Self::WriterTessellationOmitted => Severity::Warning,
            Self::WriterSourceAssociationOmitted => Severity::Info,
            Self::WriterBodyOmitted => Severity::Error,
            Self::WriterFaceOmitted => Severity::Error,
            Self::WriterGeometryOmitted => Severity::Warning,
            Self::WriterEdgeRangeAssumed => Severity::Warning,
            Self::WriterRollbackAssumed => Severity::Warning,
            Self::WriterAppearanceOmitted => Severity::Warning,
            Self::WriterHistoryUnrestored => Severity::Warning,
            Self::WriterIntegerAsReal => Severity::Info,
            Self::WriterDesignRecordOmitted => Severity::Warning,
            Self::WriterAppearanceGuidAllocated => Severity::Warning,
            Self::ScopeOwnerUnresolved
            | Self::ScopeOutputOwnerUnresolved
            | Self::OperationPoseApplied
            | Self::OperationPosePathOrdinalAssumed
            | Self::OperationPoseVersionAssumed
            | Self::OperationPoseNative
            | Self::BodylessDesignCarrier
            | Self::EmptyDesign
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
            | Self::DecalMappingAssumed
            | Self::VertexToleranceWidened => Severity::Info,
            Self::HistoryBindingBudgetExceeded
            | Self::HistoryBindingWorkExceeded
            | Self::RecipeSideSearchBudgetExceeded
            | Self::FaceRecipeMatchWorkExceeded
            | Self::ParameterOrderBudgetExceeded
            | Self::HistoryRecordFramingFailed
            | Self::MeshContainerUndecoded
            | Self::MeshOnlyDesignUntransferred
            | Self::MissingGeometryStream
            | Self::XrefCycle
            | Self::XrefMemberMissing
            | Self::XrefMemberUndecoded
            | Self::TsplineCageUndecoded
            | Self::DesignIntentUndecoded => Severity::Error,
            Self::GeometryNotTransferred | Self::TopologyNotTransferred => Severity::Blocking,
            Self::OperationPoseDisagrees
            | Self::DimensionCompanionUntyped
            | Self::BodyReferenceTailUntyped
            | Self::WorkPlaneFrameUnbound
            | Self::WorkPlaneInferredAxisAssumed
            | Self::CylinderPlacementUnbound
            | Self::CylinderLocalFrameAssumed
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
            | Self::SketchCurveTailMarkerAssumed
            | Self::SketchCurveTailConstructionAssumed
            | Self::ChamferEdgeRoleAssumed
            | Self::ChamferFixedFaceGroupAssumed
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
            | Self::CoilShortControlsAssumed
            | Self::CoilTargetUnresolved
            | Self::ConfigurationMemberUnassigned
            | Self::ConfigurationRuleUnbound
            | Self::ConfigurationParameterOverrideUnbound
            | Self::ConfigurationFeatureSuppressionUnbound
            | Self::ConfigurationSuppressionInferred
            | Self::ConfigurationActiveRowUnbound
            | Self::ActComponentLinkUnresolved
            | Self::ComponentBodyOwnerUnresolved
            | Self::ComponentBodyOwnerFallback
            | Self::StoredFrameOccurrence
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
            | Self::SketchContainerHeaderUnrecognized
            | Self::SketchGeometryGroupMemberUnlisted
            | Self::SketchGeometryGroupMemberUnbacklinked
            | Self::ComponentInsertUnplaced
            | Self::ComponentInsertOrderUnknown
            | Self::ComponentInsertContainerAssumed
            | Self::ComponentInsertLegacyFrameAssumed
            | Self::SavedFreeEdgeBodyUnowned
            | Self::LocalOccurrenceIdentityOrdinalUnresolved
            | Self::LocalOccurrenceSourceUnregistered
            | Self::LocalOccurrenceKeyRepeated
            | Self::LocalOccurrenceFirstMatrixRefused
            | Self::LocalOccurrenceEnvelopeNative
            | Self::OccurrenceVisibilityNative
            | Self::LocalComponentPathNative
            | Self::LocalOccurrenceEnvelopeSuperseded
            | Self::LocalOccurrenceTypeUnregistered
            | Self::LocalOccurrenceYieldedToRegistered
            | Self::RootInstanceDefinitionUnbound
            | Self::ComponentDefinitionRevised
            | Self::ComponentNamingSpaceUnbound
            | Self::AssemblyAlignmentPartial
            | Self::AssemblyJointOriginEnvelopeMatrixDiscarded
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
            | Self::AssemblyOperandPathWordAssumed
            | Self::AssemblyMemberJointPlacementBound
            | Self::AssemblyOperandDescentAmbiguous
            | Self::AssemblyRootPathIdentitiesNative
            | Self::DrawingDocumentOmitted
            | Self::DesignBodyBindingUnresolved
            | Self::HistoryOnlyBodyOmitted
            | Self::FlaggedBaseFeatureBodySelected
            | Self::ReferenceImageNativeRetained
            | Self::DecalNativeRetained
            | Self::DecalPlacementNative
            | Self::DecalPlacementLayoutAssumed
            | Self::DecalPlacementNormalAssumed
            | Self::DecalTargetUnresolved
            | Self::DecalImageUnresolved
            | Self::DecalDefinitionUnresolved
            | Self::EdgeReferenceLostUnrepaired
            | Self::FeatureStoredBodiesEmpty
            | Self::FeatureStoredBodiesRecipeAbsent
            | Self::FeatureHistoryBodyBounded
            | Self::FeatureHistoryBodyUndecoded
            | Self::FeatureHistoryBodyConsumedWithheld
            | Self::FeatureOwnedBodyPairingAssumed
            | Self::FeatureOutputBodyUnbound
            | Self::FeaturePatternCopyBodyOmitted
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
            | Self::FeaturePatternFeatureSeedSubstituted
            | Self::FeatureRevolvePlaneWorkPlaneAssumed
            | Self::FeatureCircularPatternPlacementNotTransferred
            | Self::FeatureCircularPatternSuppressionNotTransferred
            | Self::FeatureCircularPatternDistributionAssumed
            | Self::FeatureCircularPatternControlsAssumed
            | Self::FeaturePatternPayloadSeedUnconfirmed
            | Self::FeatureRectangularPatternDistanceFormAssumed
            | Self::FeatureRectangularPatternInactiveSymmetricDropped
            | Self::FeatureRectangularPatternDirectionOneRecordAssumed
            | Self::FeatureRectangularPatternDirectionLinksNative
            | Self::FeatureRectangularPatternConstructionNative
            | Self::FeatureRectangularPatternDesignNative
            | Self::FeatureRectangularPatternDirectionUnresolved
            | Self::FeatureRectangularPatternDirectionRecordDisagrees
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
            | Self::FeatureMeshTessellateScopeUnparsed
            | Self::FeatureMeshTessellateFieldsUninterpreted
            | Self::FeatureMeshTessellateSourceUnresolved
            | Self::FeatureMeshPlaneCutScopeUnparsed
            | Self::FeatureMeshPlaneCutTailPartial
            | Self::FeatureMeshPlaneCutPlanePartial
            | Self::FeatureMeshPlaneCutSelectorUnruled
            | Self::FeatureMeshPlaneCutFieldsUninterpreted
            | Self::FeatureMeshPlaneCutSourceUnresolved
            | Self::FeatureMeshPlaneCutSelectionUnresolved
            | Self::FeatureMeshPlaneCutMembershipAssumed
            | Self::FeatureMeshPlaneCutMatrixOrientationAssumed
            | Self::FeatureMeshPlaneCutUnitsAssumed
            | Self::FeatureMeshPlaneCutModelSpaceAssumed
            | Self::FeatureMeshPlaneCutRetainedSideAssumed
            | Self::FeatureMeshPlaneCutSplitFacesAssumed
            | Self::FeatureMeshPlaneCutUniformFillAssumed
            | Self::FeatureMeshCombineScopeUnparsed
            | Self::FeatureMeshCombineTailPartial
            | Self::FeatureMeshCombineSelectorUnruled
            | Self::FeatureMeshCombineFieldsUninterpreted
            | Self::FeatureMeshCombineSourceUnresolved
            | Self::FeatureMeshCombineSelectionUnresolved
            | Self::FeatureMeshCombineMergeAssumed
            | Self::FeatureMeshCombineLegacyBooleanAssumed
            | Self::FeatureMeshCombineKeepToolsAssumedFalse
            | Self::FeatureMeshCombineDestinationAssumedTargetComponent
            | Self::FeatureMeshCombineKeepToolsUnresolved
            | Self::FeatureMeshCombineDestinationUnresolved
            | Self::FeatureMeshConversionTailPartial
            | Self::FeatureMeshConversionMethodUnresolved
            | Self::FeatureMeshConversionMethodAssumed
            | Self::FeatureMeshConversionEnvelopeAssumed
            | Self::FeatureMeshConversionSourceUnresolved
            | Self::FeatureMeshConversionOutputUnresolved
            | Self::FeatureMeshSeparateScopeUnparsed
            | Self::FeatureMeshSeparateTailPartial
            | Self::FeatureMeshSeparateReferenceUnresolved
            | Self::FeatureMeshSeparateReferenceMismatch
            | Self::FeatureMeshSeparateSelectorUnruled
            | Self::FeatureMeshSeparateShellsMethodAssumed
            | Self::FeatureMeshSeparateFaceGroupsMethodAssumed
            | Self::FeatureMeshSeparateAllGroupsAssumed
            | Self::FeatureMeshSeparateSelectedGroupsAssumed
            | Self::FeatureMeshSeparateMultipleBodiesCarrierAssumed
            | Self::FeatureMeshSeparateMultipleBodiesPolarityAssumed
            | Self::FeatureMeshSeparateKeepBodyCarrierAssumed
            | Self::FeatureMeshSeparateKeepBodyPolarityAssumed
            | Self::FeatureMeshSeparatePrefixAssumed
            | Self::FeatureMeshSeparateOutputRoleAssumed
            | Self::FeatureMeshSeparateOutputUnresolved
            | Self::FeatureMeshSeparateSourceMembershipAssumed
            | Self::FeatureMeshSeparateSourceUnresolved
            | Self::FeatureMeshSeparateSourceFieldsUninterpreted
            | Self::FeatureMeshSeparateReferenceRolesAssumed
            | Self::FeatureMeshSeparateMembershipUnresolved
            | Self::SheetThicknessUnruled
            | Self::SheetMetalRuleControlsNative
            | Self::SheetMetalProfileNative
            | Self::BaseFlangeProfileUnresolved
            | Self::BaseFlangeThicknessUnresolved
            | Self::BaseFlangeSideUnresolved
            | Self::BaseFlangeSideAssumed
            | Self::HoleExtentUnproven
            | Self::HolePlacementUnproven
            | Self::HolePlacementPointUnselected
            | Self::FeaturePointDirectionInputsUnrepresented
            | Self::HoleReferenceDimensionsNative
            | Self::HoleParametersIndependentAssumed
            | Self::HoleShapePartialAssumed
            | Self::HoleExtentLeadingWordAssumed
            | Self::HoleTappedDiameterStored
            | Self::HoleThreadMinorDiameterAssumed
            | Self::HoleTaperTappedBoreCylindrical
            | Self::FeatureBaseFeatureNullFlagAssumed
            | Self::FeatureBaseFeatureSuffixRunOrderAssumed
            | Self::FeatureFilletPaddingAssumed
            | Self::FeatureFilletMidpointOrderAssumed
            | Self::FeatureFilletSetInputUnresolved
            | Self::FeatureFilletSetGroupUnresolved
            | Self::ThreadModeledFlagAssumed
            | Self::ThreadModeledEntriesAbsent
            | Self::ThreadConstructionUnresolved
            | Self::ThreadOffsetNative
            | Self::ThreadAngleNative
            | Self::ThreadDetailNative
            | Self::ThreadHoleNative
            | Self::ThreadFractionalSizeDiameterAssumed
            | Self::HoleStandardSizingAssumed
            | Self::HoleClearanceFieldsAssumed
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
            | Self::ParameterHeaderFlagUninterpreted
            | Self::ParameterDrivenFlagAssumed
            | Self::ParameterOwnerTrailerFlagUninterpreted
            | Self::ParameterOwnerUnstored
            | Self::ParameterUnitUntyped
            | Self::MaterialDistanceUnitUntyped
            | Self::FaceAppearanceAmbiguous
            | Self::BodyOverridePrecedenceAssumed
            | Self::BodyOverrideSuperseded
            | Self::ParameterExpressionUnbound
            | Self::ParameterReferencedDesignUnscoped
            | Self::ParameterOwnerOutsideTimeline
            | Self::ParameterOwnerUnresolvedOmitted
            | Self::ParameterNameBlank
            | Self::ConfigurationReferencedDesignUnscoped
            | Self::ConfigurationOverrideUnstated
            | Self::ConfigurationOverrideValueUnprojected
            | Self::ConfigurationScopeWithoutActiveRow
            | Self::ConfigurationPropertyUnprojected
            | Self::HistoryRollbackUnread
            | Self::HistoryRollbackBoundUnread
            | Self::CoordinateSystemConstructionNative
            | Self::CoordinateSystemConstructionDropped
            | Self::CoordinateSystemFrameUnresolved
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::HistoryDeleteFaceMemberOrderAssumed
            | Self::HistoricalEdgeMidpointsUnresolved
            | Self::HistoricalFilletEdgeInferred
            | Self::HistoricalEdgeSelectionInferred
            | Self::HistoricalEdgeSelectionDeletedSetAssumed
            | Self::HistoricalEdgeSelectionLeftoverAssumed
            | Self::HistoricalEdgeSelectionFirstSidePairAssumed
            | Self::HistoricalEdgeContinuityAssumedSharp
            | Self::HistoricalEdgeHistoryInferred
            | Self::HistoricalOutputBlobHistoryInferred
            | Self::HistoricalEdgeRecipeHistoryInferred
            | Self::HistoricalBodyRecipeHistoryInferred
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
            | Self::SketchCurveUndecoded
            | Self::SketchCurveRecordUndecoded
            | Self::SketchSurfaceUndecoded
            | Self::SketchTextUndecoded
            | Self::SketchRelationUndecoded
            | Self::SketchFolderUndecoded
            | Self::SketchOrdinalCollectionUndecoded
            | Self::SketchContainerUndecoded
            | Self::SketchGeometryGroupUndecoded
            | Self::FeatureTimelineUndecoded
            | Self::PrimaryRecordUndecoded
            | Self::ConfigurationProjectionNative
            | Self::SketchOwnerConflict
            | Self::SketchRelationOwnerUnresolved
            | Self::ConfigurationDocumentUndecoded
            | Self::BendSettingsUndecoded
            | Self::DesignBodyBoundsUndecoded
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
            | Self::FeatureExtrudeSketchTextSkipped
            | Self::FeatureExtrudeProfileGroupUnionAssumed
            | Self::FeatureExtrudeProfileSketchAbsent
            | Self::FeatureExtrudeSurfaceProfileAssumed
            | Self::FeatureExtrudeSymmetricDistanceAssumed
            | Self::FeatureExtrudeWallThicknessNative
            | Self::FeatureExtrudeAuxiliarySelectionNative
            | Self::FeatureExtrudeFaceControlAssumed
            | Self::FeatureExtrudeSketchPlacementNative
            | Self::FeatureExtrudeTargetOptionAssumed
            | Self::FeatureExtrudeOpaqueDistanceWordAssumed
            | Self::FeatureExtrudeStartFaceAbsentAssumed
            | Self::FeatureExtrudeStartFaceUnresolved
            | Self::FeatureExtrudeZeroSideTaperIgnored
            | Self::FeatureExtrudeProfileFaceOrderAssumed
            | Self::FeatureExtrudeProfileFaceSearchCapped
            | Self::FeatureExtrudeProfileCapFacesInferred
            | Self::FeatureExtrudeProfileDeletedFacesInferred
            | Self::FeatureExtrudeProfileFaceSetDisputed
            | Self::FeatureExtrudeProfileFaceCurved
            | Self::FeatureSketchProfileFlagsAssumed
            | Self::FeatureUnevaluatedStateAssumed
            | Self::FeatureNoResultActiveAssumed
            | Self::FeatureStateByteAboveOne
            | Self::FeatureProfileArrangementCapped
            | Self::FeatureProfileJoinEnvelopeCapped
            | Self::FeatureProfileCrossingRunsCapped
            | Self::FeatureProfileDensificationCapped
            | Self::FeatureProfileCrossingLoopCapped
            | Self::FeatureProfileCrossingLoopAssumed
            | Self::FeatureProfileCoincidentDirectionAssumed
            | Self::FeatureProfileCoincidentBoundaryAssumed
            | Self::FeatureProfileCrossingRunsAssumed
            | Self::FeatureProfilePartialEntryAssumed
            | Self::FeatureProfileLoopFlagsAssumed
            | Self::FeatureProfileRegionIdentitiesStale
            | Self::FeatureProfileMemberReferenceLost
            | Self::FeatureProfileTextGlyphsAssumed
            | Self::FeatureProfileTextGlyphsNative
            | Self::FeatureProfileCutRegionsAssumed
            | Self::FeatureProfileIslandAbsenceAssumed
            | Self::FeatureProfilePiecePartitionAssumed
            | Self::FeatureProfilePolylineSideAssumed
            | Self::FeatureProfileCapOutlineAssumed
            | Self::FeatureProfileCapCarrierAssumed
            | Self::FeatureProfileHoleBoundaryInferred
            | Self::FeatureProfileRegionHoleInferred
            | Self::FeatureProfileBodyCircleRingsAssumed
            | Self::FeatureSpatialProfileTransitionAssumed
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::DeleteFaceFrameControlsAssumed
            | Self::DeleteFaceGroupSelectionAssumed
            | Self::SurfaceDeleteFaceWidePrologueAssumed
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureSurfaceOffsetGroupOrdinalsAssumed
            | Self::FeatureFormSourceUnavailable
            | Self::FeatureFormDefinitionUnavailable
            | Self::FeatureFormMembershipUnresolved
            | Self::FeatureFormLegacySerializerMembershipAssumed
            | Self::FeatureFormLegacySerializerOrderAssumed
            | Self::FeatureFormCageBindingAmbiguous
            | Self::FeatureFormBodyReferencesPartialAssumed
            | Self::FeatureFormBodyRegistrationUnsupported
            | Self::FeatureFormReferenceTypeConflict
            | Self::FeatureFormForeignReferenceUnresolved
            | Self::FeatureFormTrackingOwnerConflict
            | Self::FeatureFormTrackingRunUnresolved
            | Self::FeatureFormTrackingFixedSlotsAssumed
            | Self::FeatureFormSerializerReferenceUnresolved
            | Self::FeatureFormMembershipConflict
            | Self::FeatureScaleFrameLengthFormAssumed
            | Self::FeatureScaleMatrixWordAssumed
            | Self::FeatureScaleUnlistedGroupsAssumed
            | Self::FeatureWorkAxisRuleUnassigned
            | Self::FeatureWorkAxisEndpointToleranceAssumed
            | Self::FeatureWorkAxisNonzeroParametersSolvedAssumed
            | Self::FeatureWorkAxisPartialCarrierSolvedAssumed
            | Self::FeatureWorkAxisTwoPointOperandsUnresolvedSolvedAssumed
            | Self::FeatureWorkAxisTwoPointConflictSolvedAssumed
            | Self::FeatureWorkAxisGeometryUnrepresentable
            | Self::FeatureWorkAxisBindingUnresolved
            | Self::FeatureWorkAxisLeadingBlockLayoutAssumed
            | Self::FeatureWorkAxisUnsupportedVersionLegacyLayoutAssumed
            | Self::FeatureWorkAxisLegacyLayoutAssumed
            | Self::FeatureWorkAxisCandidatesUnresolved
            | Self::FeatureWorkAxisInputNull
            | Self::FeatureWorkAxisPartialLineNative
            | Self::FeatureBodySelectionNative
            | Self::FeatureGeneratedBodyHistoryUnresolved
            | Self::FeatureReverseNormalBodyUnverified
            | Self::FeatureReverseNormalBodyProducerUnresolved
            | Self::FeatureReverseNormalLinkUnplaced
            | Self::FeatureReverseNormalLinkShared
            | Self::FeatureReverseNormalLinkOrderAssumed
            | Self::FeatureCombineBodyProducerUnresolved
            | Self::FeatureCombineOperandWriterAssumed
            | Self::FeatureCombineToolExternalFinalStateAssumed
            | Self::FeatureSilhouetteSplitPrefixUnparsed
            | Self::FeatureSilhouetteSplitModeUnresolved
            | Self::FeatureSilhouetteSplitDirectionUnresolved
            | Self::FeatureSilhouetteSplitTargetUnresolved
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
            | Self::ReplaceFaceFrameAssumed
            | Self::ReplaceFaceGroupsAggregatedAssumed
            | Self::ReplaceFaceOverlapUnresolved
            | Self::ReplaceFaceSelectionAssumed
            | Self::ReplaceFaceTangentChainAssumed
            | Self::ReplaceFaceSupportAssumed
            | Self::ReplaceFaceIdentityAssumed
            | Self::ReplaceFaceFlaggedPrefixWordAssumed
            | Self::SurfaceUntrimFrameAssumed
            | Self::SurfaceUntrimGroupSelectionAssumed
            | Self::SurfaceUntrimStoredLoopGroupsAssumed
            | Self::SurfaceUntrimLoopGroupsAggregatedAssumed
            | Self::SurfaceUntrimExtensionUnresolved
            | Self::SurfaceUntrimExtensionUnrepresentable
            | Self::SurfaceUntrimNegativeExtensionAssumed
            | Self::SurfaceUntrimPaddingAssumed
            | Self::SurfaceUntrimTrailingIdentityAssumed
            | Self::UnstitchFrameAssumed
            | Self::UnstitchGroupSelectionAssumed
            | Self::UnstitchPrologueControlsAssumed
            | Self::FeatureCombineSelectionNative
            | Self::FeatureCombineDefinitionUnresolved
            | Self::FeatureCombineMiddleListOmittedAssumed
            | Self::FeatureCombineComponentDestinationAssumed
            | Self::FeatureCombineNamedTailPrologueAssumed
            | Self::FeatureCombineToolBodyUnbound
            | Self::FeatureCombineToolExternalBodyUnbound
            | Self::FeatureCombineToolResultStateUnbound
            | Self::FeatureCombineOperandBodyUnbound
            | Self::FeatureCombineToolRecipeReplacementAssumed
            | Self::FeatureCombinePrologueAssumed
            | Self::FeatureCombineOperationEntriesAssumed
            | Self::FeatureCombineKeptToolsCutAssumed
            | Self::FeatureCombineOperandListRefused
            | Self::FeatureCombineToolConsumptionAssumed
            | Self::FeatureSplitFaceMethodUnresolved
            | Self::FeatureSplitFaceDirectionUnresolved
            | Self::FeatureSplitFaceGroupsUnresolved
            | Self::FeatureSplitFaceSelectionUnresolved
            | Self::FeatureSplitFaceSelectionProjectionFailed
            | Self::FeatureSplitFaceCarrierTypeAssumed
            | Self::FeatureSplitFaceIndexedOperandAssumed
            | Self::FeatureFixedFilletAssignmentNative
            | Self::FeatureFilletRadiusUnresolved
            | Self::FeatureFilletReferencesUnresolved
            | Self::FeatureFilletFullRoundCenterUnresolved
            | Self::FeatureFilletSetListNative
            | Self::FeatureExtrudeExtentNative
            | Self::FeatureExtrudeTerminationMissing
            | Self::FeatureRevolveLayoutAssumed
            | Self::FeatureRevolveFrameUnmatched
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
            | Self::FeatureLoftSolidInferred
            | Self::FeatureSurfaceExtendMethodUnresolved
            | Self::FeatureSurfaceExtendEdgesAsFaces
            | Self::FeatureSplitToolExtensionAssumed
            | Self::FeatureSplitToolExtensionUnresolved
            | Self::FeatureSplitBodyFaceToolGroupAssumed
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
            | Self::FeatureShellRemovedFacesAssumed
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
            | Self::FeatureBoundaryFillCellsNative
            | Self::FeatureBoundaryFillCellsUnresolved
            | Self::FeatureBoundaryFillOperationUnresolved
            | Self::FeatureBoundaryFillToolsUnresolved
            | Self::FeatureBoundaryFillTargetsUnresolved
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
            | Self::MeshComponentFlagUninterpreted
            | Self::MeshOperationRecordsNative
            | Self::MeshFaceGroupsAssumed
            | Self::MeshBodyWordsNative
            | Self::MeshCollectionWordsNative
            | Self::XrefLineageDisagrees
            | Self::XrefIdentityBlank
            | Self::XrefArchiveIdentityDisagrees
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
            | Self::LocalOccurrenceMarkedPreludeAssumed
            | Self::LocalOccurrenceObjectPreludeAssumed
            | Self::PlacementChildrenDiffer
            | Self::InstanceReferenceShared
            | Self::XrefContainedPoseDisagrees
            | Self::XrefOwnedPoseDisagrees
            | Self::XrefPlacementMetadataMissing
            | Self::XrefPlacementSuperseded
            | Self::XrefOwnerUnresolved
            | Self::JointPlacedOccurrenceUnsolved
            | Self::MeshVertexPrecisionReduced
            | Self::MeshBodyIdentityAssumed
            | Self::SurfaceShapeNotDecoded
            | Self::ProceduralCurveUndecoded
            | Self::ProceduralSupportChartUnresolved
            | Self::SpringSupportPcurveUnresolved
            | Self::PcurveUndecoded
            | Self::BlendSupportPartial
            | Self::SolvedRecordUntyped
            | Self::MaterialNotTransferred
            | Self::TsplineRecordUntyped
            | Self::TsplineRadialMapUnresolved
            | Self::SourceDialectUnverified
            | Self::KernelDialectUnverified
            | Self::KernelCarrierUnparseable
            | Self::DialectLayerCollision
            | Self::TextHeaderNoncanonical
            | Self::KernelHeaderToleranceUnresolved
            | Self::KernelHeaderMetadataUnresolved
            | Self::ManifestMetadataUnresolved
            | Self::UnindexedRecordsRetained
            | Self::BodyDisplayNameAmbiguous
            | Self::WorkPlaneDisplayNameAmbiguous
            | Self::WorkPlaneDisplayNameUndecoded
            | Self::TopologyFaceBoundaryUnstated => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::FeatureBodyHistoryIncomplete
            | Self::FeatureBodyHistoryOwnerUnresolved
            | Self::FeatureBodyHistoryUnread => LossTaxonomy::FeatureHistoryRetained,
            Self::SceneOverrideRefused
            | Self::SettingsUpAxisUnrecognized
            | Self::SettingsUpAxisRecordUnreadable => LossTaxonomy::RecordNotTyped,
            Self::OccurrenceVisibilityNative => LossTaxonomy::AttributesNotTransferred,
            Self::SceneOverrideRefused | Self::SceneVisibilityNative => {
                LossTaxonomy::RecordNotTyped
            }
            Self::BendStationaryRoleAssumed
            | Self::BendAngleUnresolved
            | Self::BendRadiusFirstAssumed
            | Self::BendLineOwnerOrderAssumed
            | Self::BendStationarySelectionNative
            | Self::BendRuleRadiusUnresolved
            | Self::BendAngleOwnerBacklinkAssumed
            | Self::BendCachedOwnerAngleAssumed
            | Self::EdgeReferenceEnvelopeUnresolved
            | Self::EdgeParameterOwnerBacklinkAssumed
            | Self::EdgeCachedParameterAssumed
            | Self::EdgeParameterUnitAssumed
            | Self::EdgeParameterUnresolved
            | Self::EdgeStoredRadiusInsideAssumed
            | Self::EdgeRadiusUnresolved
            | Self::EdgeSelectionNative
            | Self::EdgeBoundaryReferenceNative
            | Self::EdgeBoundaryScopeReferenceAssumed
            | Self::EdgeFullWidthAssumed
            | Self::EdgeSignedOffsetsAssumed
            | Self::EdgeDatumUnresolved
            | Self::EdgePositionUnresolved
            | Self::EdgeWidthUnresolved
            | Self::EdgeHeightTargetNative
            | Self::EdgeHeightTargetScopeAssumed
            | Self::DraftPartingControlsPartial
            | Self::DraftNeutralAnchorAssumed
            | Self::DraftSlopeOutOfRange
            | Self::BaseFaceRegionsNative
            | Self::BaseFaceSourceCarrierUnresolved
            | Self::BaseFaceThicknessUnresolved
            | Self::BaseFaceMaterialSideUnresolved
            | Self::BaseFaceResultChoiceUnresolved
            | Self::ContourVectorOwnerDistanceRoleAssumed
            | Self::ContourProfileNative
            | Self::ContourDefaultSideOneLeftAssumed
            | Self::ContourEnvelopeUnresolved
            | Self::ContourOwnerVectorDirectionAssumed
            | Self::ContourOperationFourBodyAssumed
            | Self::ContourResultUnresolved
            | Self::ContourSettingsRuleReferenceAssumed
            | Self::ContourRuleFirstRadiusAssumed
            | Self::ContourMaterialScalarFactorAssumed
            | Self::ContourInlineThicknessAssumed
            | Self::ContourRadiusDatabaseUnitAssumed
            | Self::ContourDistanceDatabaseUnitAssumed
            | Self::ContourCachedOwnerDistanceAssumed
            | Self::ContourSingleDistanceOneSidedAssumed
            | Self::ContourDistanceUnresolved
            | Self::ContourFirstRadiusOverrideAssumed
            | Self::ContourDirectionUnresolved
            | Self::ContourThicknessUnresolved
            | Self::ContourRadiusUnresolved
            | Self::ContourRuleFactorUnresolved
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
            Self::WriterProceduralReduced => LossTaxonomy::ProceduralReduced,
            Self::WriterPcurveOmitted => LossTaxonomy::PcurveOmitted,
            Self::WriterSubdOmitted => LossTaxonomy::SubdOmitted,
            Self::WriterAssetOmitted => LossTaxonomy::AssetNotTransferred,
            Self::WriterJointOmitted => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::WriterParameterOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterSketchContentOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterSketchSupportSubstituted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterConfigurationOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterTessellationOmitted => LossTaxonomy::TessellationOmitted,
            Self::WriterSourceAssociationOmitted => LossTaxonomy::SourceAssociationOmitted,
            Self::WriterBodyOmitted => LossTaxonomy::TopologyNotTransferred,
            Self::WriterFaceOmitted => LossTaxonomy::TopologyNotTransferred,
            Self::WriterGeometryOmitted => LossTaxonomy::GeometryNotTransferred,
            Self::WriterEdgeRangeAssumed => LossTaxonomy::TopologyGaugeSubstituted,
            Self::WriterRollbackAssumed => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterAppearanceOmitted => LossTaxonomy::MaterialNotTransferred,
            Self::WriterHistoryUnrestored => LossTaxonomy::PreservedSourceUnavailable,
            Self::WriterIntegerAsReal => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterDesignRecordOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterAppearanceGuidAllocated => LossTaxonomy::MetadataNotTransferred,
            Self::DimensionCompanionUntyped
            | Self::BodyReferenceTailUntyped
            | Self::PlacementConstructionUntyped
            | Self::ConstructionTrackingPathUntyped
            | Self::ParameterOwnerUnparsed
            | Self::ParameterRecordUnparsed
            | Self::HistoryRecordFramingFailed
            | Self::SolvedRecordUntyped
            | Self::KernelCarrierUnparseable
            | Self::FeatureTimelineUndecoded
            | Self::PrimaryRecordUndecoded
            | Self::ConfigurationDocumentUndecoded
            | Self::BendSettingsUndecoded
            | Self::TsplineRecordUntyped => LossTaxonomy::RecordNotTyped,
            Self::ScopeOwnerUnresolved
            | Self::ConstructionClosingFlagAssumed
            | Self::SketchCurveTailMarkerAssumed
            | Self::SketchCurveTailConstructionAssumed
            | Self::ChamferEdgeRoleAssumed
            | Self::ChamferFixedFaceGroupAssumed
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
            | Self::CoilShortControlsAssumed
            | Self::CoilTargetUnresolved
            | Self::OperationPoseApplied
            | Self::OperationPosePathOrdinalAssumed
            | Self::OperationPoseVersionAssumed
            | Self::OperationPoseDisagrees
            | Self::OperationPoseNative
            | Self::ConfigurationMemberUnassigned
            | Self::ConfigurationProjectionNative
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
            | Self::LocalOccurrenceMarkedPreludeAssumed
            | Self::LocalOccurrenceObjectPreludeAssumed
            | Self::PlacementChildrenDiffer
            | Self::InstanceReferenceShared
            | Self::XrefContainedPose
            | Self::XrefContainedPoseDisagrees
            | Self::XrefContainedPoseNative
            | Self::XrefOwnedPose
            | Self::XrefOwnedPoseDisagrees
            | Self::XrefOwnedPoseNative
            | Self::MeshGuidBytesUninterpreted
            | Self::MeshComponentFlagUninterpreted
            | Self::MeshOperationRecordsNative
            | Self::MeshFaceGroupsAssumed
            | Self::MeshBodyWordsNative
            | Self::MeshCollectionWordsNative
            | Self::MeshBodyBytesUninterpreted
            | Self::MeshWrapperBytesUninterpreted
            | Self::MeshOwnerBytesUninterpreted
            | Self::MeshScopePrefixUninterpreted
            | Self::XrefLineageDisagrees
            | Self::XrefIdentityBlank
            | Self::XrefArchiveIdentityDisagrees
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
            | Self::FaceRecipeMatchWorkExceeded
            | Self::ParameterOrderBudgetExceeded
            | Self::FeatureStoredBodiesEmpty
            | Self::FeatureStoredBodiesRecipeAbsent
            | Self::FeatureHistoryBodyBounded
            | Self::FeatureHistoryBodyUndecoded
            | Self::FeatureHistoryBodyConsumedWithheld
            | Self::FeatureOwnedBodyPairingAssumed
            | Self::FeatureOutputBodyUnbound
            | Self::FeaturePatternCopyBodyOmitted
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
            | Self::FeaturePatternFeatureSeedSubstituted
            | Self::FeatureRevolvePlaneWorkPlaneAssumed
            | Self::FeatureCircularPatternPlacementNotTransferred
            | Self::FeatureCircularPatternSuppressionNotTransferred
            | Self::FeatureCircularPatternDistributionAssumed
            | Self::FeatureCircularPatternControlsAssumed
            | Self::FeaturePatternPayloadSeedUnconfirmed
            | Self::FeatureRectangularPatternDistanceFormAssumed
            | Self::FeatureRectangularPatternInactiveSymmetricDropped
            | Self::FeatureRectangularPatternDirectionOneRecordAssumed
            | Self::FeatureRectangularPatternDirectionLinksNative
            | Self::FeatureRectangularPatternConstructionNative
            | Self::FeatureRectangularPatternDesignNative
            | Self::FeatureRectangularPatternDirectionUnresolved
            | Self::FeatureRectangularPatternDirectionRecordDisagrees
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
            | Self::FeatureMeshTessellateScopeUnparsed
            | Self::FeatureMeshTessellateFieldsUninterpreted
            | Self::FeatureMeshTessellateSourceUnresolved
            | Self::FeatureMeshPlaneCutScopeUnparsed
            | Self::FeatureMeshPlaneCutTailPartial
            | Self::FeatureMeshPlaneCutPlanePartial
            | Self::FeatureMeshPlaneCutSelectorUnruled
            | Self::FeatureMeshPlaneCutFieldsUninterpreted
            | Self::FeatureMeshPlaneCutSourceUnresolved
            | Self::FeatureMeshPlaneCutSelectionUnresolved
            | Self::FeatureMeshPlaneCutMembershipAssumed
            | Self::FeatureMeshPlaneCutMatrixOrientationAssumed
            | Self::FeatureMeshPlaneCutUnitsAssumed
            | Self::FeatureMeshPlaneCutModelSpaceAssumed
            | Self::FeatureMeshPlaneCutRetainedSideAssumed
            | Self::FeatureMeshPlaneCutSplitFacesAssumed
            | Self::FeatureMeshPlaneCutUniformFillAssumed
            | Self::FeatureMeshCombineScopeUnparsed
            | Self::FeatureMeshCombineTailPartial
            | Self::FeatureMeshCombineSelectorUnruled
            | Self::FeatureMeshCombineFieldsUninterpreted
            | Self::FeatureMeshCombineSourceUnresolved
            | Self::FeatureMeshCombineSelectionUnresolved
            | Self::FeatureMeshCombineMergeAssumed
            | Self::FeatureMeshCombineLegacyBooleanAssumed
            | Self::FeatureMeshCombineKeepToolsAssumedFalse
            | Self::FeatureMeshCombineDestinationAssumedTargetComponent
            | Self::FeatureMeshCombineKeepToolsUnresolved
            | Self::FeatureMeshCombineDestinationUnresolved
            | Self::FeatureMeshConversionTailPartial
            | Self::FeatureMeshConversionMethodUnresolved
            | Self::FeatureMeshConversionMethodAssumed
            | Self::FeatureMeshConversionEnvelopeAssumed
            | Self::FeatureMeshConversionSourceUnresolved
            | Self::FeatureMeshConversionOutputUnresolved
            | Self::FeatureMeshSeparateScopeUnparsed
            | Self::FeatureMeshSeparateTailPartial
            | Self::FeatureMeshSeparateReferenceUnresolved
            | Self::FeatureMeshSeparateReferenceMismatch
            | Self::FeatureMeshSeparateSelectorUnruled
            | Self::FeatureMeshSeparateShellsMethodAssumed
            | Self::FeatureMeshSeparateFaceGroupsMethodAssumed
            | Self::FeatureMeshSeparateAllGroupsAssumed
            | Self::FeatureMeshSeparateSelectedGroupsAssumed
            | Self::FeatureMeshSeparateMultipleBodiesCarrierAssumed
            | Self::FeatureMeshSeparateMultipleBodiesPolarityAssumed
            | Self::FeatureMeshSeparateKeepBodyCarrierAssumed
            | Self::FeatureMeshSeparateKeepBodyPolarityAssumed
            | Self::FeatureMeshSeparatePrefixAssumed
            | Self::FeatureMeshSeparateOutputRoleAssumed
            | Self::FeatureMeshSeparateOutputUnresolved
            | Self::FeatureMeshSeparateSourceMembershipAssumed
            | Self::FeatureMeshSeparateSourceUnresolved
            | Self::FeatureMeshSeparateSourceFieldsUninterpreted
            | Self::FeatureMeshSeparateReferenceRolesAssumed
            | Self::FeatureMeshSeparateMembershipUnresolved
            | Self::ThreadLiveFaceUnproven
            | Self::SheetThicknessUnruled
            | Self::SheetMetalRuleControlsNative
            | Self::SheetMetalProfileNative
            | Self::BaseFlangeProfileUnresolved
            | Self::BaseFlangeThicknessUnresolved
            | Self::BaseFlangeSideUnresolved
            | Self::BaseFlangeSideAssumed
            | Self::WorkPlaneFrameUnbound
            | Self::WorkPlaneInferredAxisAssumed
            | Self::CylinderPlacementUnbound
            | Self::CylinderLocalFrameAssumed
            | Self::SpherePrefixFramingAssumed
            | Self::SpherePlacementFlagsAssumed
            | Self::SphereLocalMatrixFramingAssumed
            | Self::SphereLocalFrameAssumed
            | Self::SpherePlacementFrameAssumed
            | Self::SphereDiameterOwnerAssumed
            | Self::SphereDiameterSourceAssumed
            | Self::HoleExtentUnproven
            | Self::HolePlacementUnproven
            | Self::HolePlacementPointUnselected
            | Self::FeaturePointDirectionInputsUnrepresented
            | Self::HoleReferenceDimensionsNative
            | Self::HoleParametersIndependentAssumed
            | Self::HoleShapePartialAssumed
            | Self::HoleExtentLeadingWordAssumed
            | Self::HoleTappedDiameterStored
            | Self::HoleThreadMinorDiameterAssumed
            | Self::HoleTaperTappedBoreCylindrical
            | Self::FeatureBaseFeatureNullFlagAssumed
            | Self::FeatureBaseFeatureSuffixRunOrderAssumed
            | Self::FeatureFilletPaddingAssumed
            | Self::FeatureFilletMidpointOrderAssumed
            | Self::FeatureFilletSetInputUnresolved
            | Self::FeatureFilletSetGroupUnresolved
            | Self::ThreadModeledFlagAssumed
            | Self::ThreadModeledEntriesAbsent
            | Self::ThreadConstructionUnresolved
            | Self::ThreadOffsetNative
            | Self::ThreadAngleNative
            | Self::ThreadDetailNative
            | Self::ThreadHoleNative
            | Self::ThreadFractionalSizeDiameterAssumed
            | Self::HoleStandardSizingAssumed
            | Self::HoleClearanceFieldsAssumed
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
            | Self::ParameterHeaderFlagUninterpreted
            | Self::ParameterDrivenFlagAssumed
            | Self::ParameterOwnerTrailerFlagUninterpreted
            | Self::ParameterOwnerUnstored
            | Self::ParameterUnitUntyped
            | Self::ParameterExpressionUnbound
            | Self::ParameterReferencedDesignUnscoped
            | Self::ParameterOwnerOutsideTimeline
            | Self::ParameterOwnerUnresolvedOmitted
            | Self::ParameterNameBlank
            | Self::ConfigurationReferencedDesignUnscoped
            | Self::ConfigurationOverrideUnstated
            | Self::ConfigurationOverrideValueUnprojected
            | Self::ConfigurationScopeWithoutActiveRow
            | Self::ConfigurationPropertyUnprojected
            | Self::HistoryRollbackUnread
            | Self::HistoryRollbackBoundUnread
            | Self::CoordinateSystemConstructionNative
            | Self::CoordinateSystemConstructionDropped
            | Self::CoordinateSystemFrameUnresolved
            | Self::HistoryDependencyUnprojected
            | Self::HistoryDependencyAmbiguous
            | Self::HistoricalFaceSelectionUnbound
            | Self::HistoryDeleteFaceMemberOrderAssumed
            | Self::HistoricalEdgeMidpointsUnresolved
            | Self::HistoricalFilletEdgeInferred
            | Self::HistoricalEdgeSelectionInferred
            | Self::HistoricalEdgeSelectionDeletedSetAssumed
            | Self::HistoricalEdgeSelectionLeftoverAssumed
            | Self::HistoricalEdgeSelectionFirstSidePairAssumed
            | Self::HistoricalEdgeContinuityAssumedSharp
            | Self::HistoricalEdgeHistoryInferred
            | Self::HistoricalOutputBlobHistoryInferred
            | Self::HistoricalEdgeRecipeHistoryInferred
            | Self::HistoricalBodyRecipeHistoryInferred
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
            | Self::FeatureExtrudeSketchTextSkipped
            | Self::FeatureExtrudeProfileGroupUnionAssumed
            | Self::FeatureExtrudeProfileSketchAbsent
            | Self::FeatureExtrudeSurfaceProfileAssumed
            | Self::FeatureExtrudeSymmetricDistanceAssumed
            | Self::FeatureExtrudeWallThicknessNative
            | Self::FeatureExtrudeAuxiliarySelectionNative
            | Self::FeatureExtrudeFaceControlAssumed
            | Self::FeatureExtrudeSketchPlacementNative
            | Self::FeatureExtrudeTargetOptionAssumed
            | Self::FeatureExtrudeOpaqueDistanceWordAssumed
            | Self::FeatureExtrudeStartFaceAbsentAssumed
            | Self::FeatureExtrudeStartFaceUnresolved
            | Self::FeatureExtrudeZeroSideTaperIgnored
            | Self::FeatureExtrudeProfileFaceOrderAssumed
            | Self::FeatureExtrudeProfileFaceSearchCapped
            | Self::FeatureExtrudeProfileCapFacesInferred
            | Self::FeatureExtrudeProfileDeletedFacesInferred
            | Self::FeatureExtrudeProfileFaceSetDisputed
            | Self::FeatureExtrudeProfileFaceCurved
            | Self::FeatureSketchProfileFlagsAssumed
            | Self::FeatureUnevaluatedStateAssumed
            | Self::FeatureNoResultActiveAssumed
            | Self::FeatureStateByteAboveOne
            | Self::FeatureProfileArrangementCapped
            | Self::FeatureProfileJoinEnvelopeCapped
            | Self::FeatureProfileCrossingRunsCapped
            | Self::FeatureProfileDensificationCapped
            | Self::FeatureProfileCrossingLoopCapped
            | Self::FeatureProfileCrossingLoopAssumed
            | Self::FeatureProfileCoincidentDirectionAssumed
            | Self::FeatureProfileCoincidentBoundaryAssumed
            | Self::FeatureProfileCrossingRunsAssumed
            | Self::FeatureProfilePartialEntryAssumed
            | Self::FeatureProfileLoopFlagsAssumed
            | Self::FeatureProfileRegionIdentitiesStale
            | Self::FeatureProfileMemberReferenceLost
            | Self::FeatureProfileTextGlyphsAssumed
            | Self::FeatureProfileTextGlyphsNative
            | Self::FeatureProfileCutRegionsAssumed
            | Self::FeatureProfileIslandAbsenceAssumed
            | Self::FeatureProfilePiecePartitionAssumed
            | Self::FeatureProfilePolylineSideAssumed
            | Self::FeatureProfileCapOutlineAssumed
            | Self::FeatureProfileCapCarrierAssumed
            | Self::FeatureProfileHoleBoundaryInferred
            | Self::FeatureProfileRegionHoleInferred
            | Self::FeatureProfileBodyCircleRingsAssumed
            | Self::FeatureSpatialProfileTransitionAssumed
            | Self::FeaturePathSelectionNative
            | Self::FeatureFaceSelectionNative
            | Self::DeleteFaceFrameControlsAssumed
            | Self::DeleteFaceGroupSelectionAssumed
            | Self::SurfaceDeleteFaceWidePrologueAssumed
            | Self::FeatureFaceSelectionActiveSubstituted
            | Self::FeatureSurfaceOffsetGroupOrdinalsAssumed
            | Self::FeatureFormSourceUnavailable
            | Self::FeatureFormDefinitionUnavailable
            | Self::FeatureFormMembershipUnresolved
            | Self::FeatureFormLegacySerializerMembershipAssumed
            | Self::FeatureFormLegacySerializerOrderAssumed
            | Self::FeatureFormCageBindingAmbiguous
            | Self::FeatureFormBodyReferencesPartialAssumed
            | Self::FeatureFormBodyRegistrationUnsupported
            | Self::FeatureFormReferenceTypeConflict
            | Self::FeatureFormForeignReferenceUnresolved
            | Self::FeatureFormTrackingOwnerConflict
            | Self::FeatureFormTrackingRunUnresolved
            | Self::FeatureFormTrackingFixedSlotsAssumed
            | Self::FeatureFormSerializerReferenceUnresolved
            | Self::FeatureFormMembershipConflict
            | Self::FeatureScaleFrameLengthFormAssumed
            | Self::FeatureScaleMatrixWordAssumed
            | Self::FeatureScaleUnlistedGroupsAssumed
            | Self::FeatureWorkAxisRuleUnassigned
            | Self::FeatureWorkAxisEndpointToleranceAssumed
            | Self::FeatureWorkAxisNonzeroParametersSolvedAssumed
            | Self::FeatureWorkAxisPartialCarrierSolvedAssumed
            | Self::FeatureWorkAxisTwoPointOperandsUnresolvedSolvedAssumed
            | Self::FeatureWorkAxisTwoPointConflictSolvedAssumed
            | Self::FeatureWorkAxisGeometryUnrepresentable
            | Self::FeatureWorkAxisBindingUnresolved
            | Self::FeatureWorkAxisLeadingBlockLayoutAssumed
            | Self::FeatureWorkAxisUnsupportedVersionLegacyLayoutAssumed
            | Self::FeatureWorkAxisLegacyLayoutAssumed
            | Self::FeatureWorkAxisCandidatesUnresolved
            | Self::FeatureWorkAxisInputNull
            | Self::FeatureWorkAxisPartialLineNative
            | Self::FeatureBodySelectionNative
            | Self::FeatureGeneratedBodyHistoryUnresolved
            | Self::FeatureReverseNormalBodyUnverified
            | Self::FeatureReverseNormalBodyProducerUnresolved
            | Self::FeatureReverseNormalLinkUnplaced
            | Self::FeatureReverseNormalLinkShared
            | Self::FeatureReverseNormalLinkOrderAssumed
            | Self::FeatureCombineBodyProducerUnresolved
            | Self::FeatureCombineOperandWriterAssumed
            | Self::FeatureCombineToolExternalFinalStateAssumed
            | Self::FeatureSilhouetteSplitPrefixUnparsed
            | Self::FeatureSilhouetteSplitModeUnresolved
            | Self::FeatureSilhouetteSplitDirectionUnresolved
            | Self::FeatureSilhouetteSplitTargetUnresolved
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
            | Self::ReplaceFaceFrameAssumed
            | Self::ReplaceFaceGroupsAggregatedAssumed
            | Self::ReplaceFaceOverlapUnresolved
            | Self::ReplaceFaceSelectionAssumed
            | Self::ReplaceFaceTangentChainAssumed
            | Self::ReplaceFaceSupportAssumed
            | Self::ReplaceFaceIdentityAssumed
            | Self::ReplaceFaceFlaggedPrefixWordAssumed
            | Self::SurfaceUntrimFrameAssumed
            | Self::SurfaceUntrimGroupSelectionAssumed
            | Self::SurfaceUntrimStoredLoopGroupsAssumed
            | Self::SurfaceUntrimLoopGroupsAggregatedAssumed
            | Self::SurfaceUntrimExtensionUnresolved
            | Self::SurfaceUntrimExtensionUnrepresentable
            | Self::SurfaceUntrimNegativeExtensionAssumed
            | Self::SurfaceUntrimPaddingAssumed
            | Self::SurfaceUntrimTrailingIdentityAssumed
            | Self::UnstitchFrameAssumed
            | Self::UnstitchGroupSelectionAssumed
            | Self::UnstitchPrologueControlsAssumed
            | Self::FeatureCombineSelectionNative
            | Self::FeatureCombineDefinitionUnresolved
            | Self::FeatureCombineMiddleListOmittedAssumed
            | Self::FeatureCombineComponentDestinationAssumed
            | Self::FeatureCombineNamedTailPrologueAssumed
            | Self::FeatureCombineToolBodyUnbound
            | Self::FeatureCombineToolExternalBodyUnbound
            | Self::FeatureCombineToolResultStateUnbound
            | Self::FeatureCombineOperandBodyUnbound
            | Self::FeatureCombineToolRecipeReplacementAssumed
            | Self::FeatureCombinePrologueAssumed
            | Self::FeatureCombineOperationEntriesAssumed
            | Self::FeatureCombineKeptToolsCutAssumed
            | Self::FeatureCombineOperandListRefused
            | Self::FeatureCombineToolConsumptionAssumed
            | Self::FeatureSplitFaceMethodUnresolved
            | Self::FeatureSplitFaceDirectionUnresolved
            | Self::FeatureSplitFaceGroupsUnresolved
            | Self::FeatureSplitFaceSelectionUnresolved
            | Self::FeatureSplitFaceSelectionProjectionFailed
            | Self::FeatureSplitFaceCarrierTypeAssumed
            | Self::FeatureSplitFaceIndexedOperandAssumed
            | Self::FeatureFixedFilletAssignmentNative
            | Self::FeatureFilletRadiusUnresolved
            | Self::FeatureFilletReferencesUnresolved
            | Self::FeatureFilletFullRoundCenterUnresolved
            | Self::FeatureFilletSetListNative
            | Self::FeatureExtrudeExtentNative
            | Self::FeatureExtrudeTerminationMissing
            | Self::FeatureRevolveLayoutAssumed
            | Self::FeatureRevolveFrameUnmatched
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
            | Self::FeatureLoftSolidInferred
            | Self::FeatureSurfaceExtendMethodUnresolved
            | Self::FeatureSurfaceExtendEdgesAsFaces
            | Self::FeatureSplitToolExtensionAssumed
            | Self::FeatureSplitToolExtensionUnresolved
            | Self::FeatureSplitBodyFaceToolGroupAssumed
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
            | Self::FeatureShellRemovedFacesAssumed
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
            | Self::FeatureBoundaryFillCellsNative
            | Self::FeatureBoundaryFillCellsUnresolved
            | Self::FeatureBoundaryFillOperationUnresolved
            | Self::FeatureBoundaryFillToolsUnresolved
            | Self::FeatureBoundaryFillTargetsUnresolved
            | Self::FeatureFaceOperandFallbackContradicted
            | Self::FeatureExtrudeTargetDisputed
            | Self::FeatureEdgeSelectionNative
            | Self::FeatureEdgeOperandUnresolved
            | Self::FeatureEdgeSelectionLost
            | Self::FeatureOccurrencePathUnresolved
            | Self::FeaturePatternInstanceSearchCapped
            | Self::AssemblyAlignmentPartial
            | Self::AssemblyJointOriginEnvelopeMatrixDiscarded
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
            | Self::AssemblyOperandPathWordAssumed
            | Self::AssemblyOperandDescentAmbiguous
            | Self::AssemblyRootPathIdentitiesNative
            | Self::HistoryOnlyBodyOmitted => LossTaxonomy::FeatureHistoryRetained,
            Self::MaterialDistanceUnitUntyped => LossTaxonomy::MaterialNotTransferred,
            Self::FaceAppearanceAmbiguous
            | Self::BodyOverridePrecedenceAssumed
            | Self::BodyOverrideSuperseded => LossTaxonomy::AppearanceReduced,
            Self::XrefOwnerUnresolved
            | Self::ComponentBodyOwnerUnresolved
            | Self::ComponentBodyOwnerFallback
            | Self::StoredFrameOccurrence
            | Self::LocalOccurrenceOwnerUnbound
            | Self::LocalOccurrenceOwnerCycle
            | Self::LocalOccurrenceExpansionLimit
            | Self::LocalOccurrenceUnplaced
            | Self::ComponentInsertUnplaced
            | Self::ComponentInsertOrderUnknown
            | Self::ComponentInsertContainerAssumed
            | Self::ComponentInsertLegacyFrameAssumed
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
            | Self::ComponentNamingSpaceUnbound
            | Self::AssemblyMemberJointPlacementBound => {
                LossTaxonomy::AssemblyPlacementsNotTransferred
            }
            Self::SketchOwnerUnresolved
            | Self::SketchMembershipConflict
            | Self::SketchFolderClosingReference
            | Self::SketchFolderUnprojected
            | Self::SketchContainerHeaderUnrecognized
            | Self::SketchFolderUndecoded
            | Self::SketchOrdinalCollectionUndecoded
            | Self::SketchContainerUndecoded
            | Self::SketchGeometryGroupUndecoded
            | Self::SketchOwnerConflict
            | Self::SketchRelationOwnerUnresolved
            | Self::SketchGeometryGroupMemberUnlisted
            | Self::SketchGeometryGroupMemberUnbacklinked
            | Self::DesignBodyBindingUnresolved
            | Self::PcurveUndecoded => LossTaxonomy::ReferenceGraphNotClosed,
            Self::SketchNameComposedFromFeatureOrdinal
            | Self::ReferenceImageNativeRetained
            | Self::ReferenceImageFlagAssumed
            | Self::DecalNativeRetained
            | Self::DecalMappingAssumed
            | Self::DecalPlacementNative
            | Self::DecalPlacementLayoutAssumed
            | Self::DecalPlacementNormalAssumed
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
            | Self::MeshOnlyDesignUntransferred
            | Self::TsplineCageUndecoded
            | Self::SketchPointUndecoded
            | Self::SketchCurveUndecoded
            | Self::SketchCurveRecordUndecoded
            | Self::SketchSurfaceUndecoded
            | Self::DesignBodyBoundsUndecoded
            | Self::SketchTextUndecoded
            | Self::SketchRelationUndecoded => LossTaxonomy::GeometryNotTransferred,
            Self::MeshContainerUnjoined | Self::MeshContainerMissing => {
                LossTaxonomy::AssetNotTransferred
            }
            Self::MeshContainerUndecoded
            | Self::MeshBodyIdentityAssumed
            | Self::DialectLayerCollision
            | Self::LocalOccurrenceYieldedToRegistered
            | Self::FlaggedBaseFeatureBodySelected
            | Self::DesignIntentUndecoded => LossTaxonomy::DecodeDiagnostic,
            Self::MeshVertexPrecisionReduced => LossTaxonomy::MeshVertexPrecision,
            Self::BodylessDesignCarrier
            | Self::EmptyDesign
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
            Self::TopologyFaceBoundaryUnstated => LossTaxonomy::TopologyNotTransferred,
            Self::MissingGeometryStream => LossTaxonomy::MissingGeometryStream,
            Self::SourceDialectUnverified | Self::KernelDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::TextHeaderNoncanonical => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::KernelHeaderToleranceUnresolved => LossTaxonomy::GeometryNotTransferred,
            Self::KernelHeaderMetadataUnresolved => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::ManifestMetadataUnresolved => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::UnindexedRecordsRetained => LossTaxonomy::RecordNotTyped,
        }
    }

    /// The strict floor `f3d.md` §4 states: each code that records a Design
    /// record the decoder keeps with less content pins Warning, and so does
    /// the design-intent failure that the decoder keeps as a record.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::DesignIntentUndecoded
            | Self::SketchCurveUndecoded
            | Self::SketchContainerUndecoded
            | Self::ConfigurationProjectionNative
            | Self::SketchOwnerConflict
            | Self::SketchRelationOwnerUnresolved => Some(Severity::Warning),
            _ => None,
        }
    }

    /// Namespaced [`LossKind`] for this local code.
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
    /// code.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::F3dLossCode;
    use std::collections::BTreeSet;

    /// The codes that record a Design record the decoder keeps with less
    /// content: the design-intent failure record, a sketch curve without its
    /// geometry, a sketch without its container row, configurations left
    /// native, and sketch geometry or a relation with an unresolved owner.
    const KEPT_DESIGN_RECORD_CODES: [F3dLossCode; 6] = [
        F3dLossCode::DesignIntentUndecoded,
        F3dLossCode::SketchCurveUndecoded,
        F3dLossCode::SketchContainerUndecoded,
        F3dLossCode::ConfigurationProjectionNative,
        F3dLossCode::SketchOwnerConflict,
        F3dLossCode::SketchRelationOwnerUnresolved,
    ];

    /// The codes that record only a Design record the decoder gives up. The
    /// record's `record.unreadable` refuses a pinned decode, so these codes
    /// have no floor.
    const GIVEN_UP_DESIGN_RECORD_CODES: [F3dLossCode; 12] = [
        F3dLossCode::SketchCurveRecordUndecoded,
        F3dLossCode::SketchSurfaceUndecoded,
        F3dLossCode::SketchTextUndecoded,
        F3dLossCode::SketchRelationUndecoded,
        F3dLossCode::SketchFolderUndecoded,
        F3dLossCode::SketchOrdinalCollectionUndecoded,
        F3dLossCode::SketchGeometryGroupUndecoded,
        F3dLossCode::FeatureTimelineUndecoded,
        F3dLossCode::PrimaryRecordUndecoded,
        F3dLossCode::ConfigurationDocumentUndecoded,
        F3dLossCode::BendSettingsUndecoded,
        F3dLossCode::DesignBodyBoundsUndecoded,
    ];

    #[test]
    fn kept_design_records_keep_strict_refusal() {
        for code in KEPT_DESIGN_RECORD_CODES {
            assert!(
                cadmpeg_ir::report::strictness::Strictness {
                    pinned: true,
                    ..Default::default()
                }
                .refuses(&code.note("synthetic kept Design record"), false),
                "{}",
                code.kind().local_code()
            );
        }
    }

    #[test]
    fn given_up_design_record_codes_have_no_floor() {
        for code in GIVEN_UP_DESIGN_RECORD_CODES {
            assert_eq!(
                code.kind().strict_floor(),
                None,
                "{}",
                code.kind().local_code()
            );
        }
    }

    #[test]
    fn design_record_codes_are_registered_once() {
        let mut seen = BTreeSet::new();
        for code in KEPT_DESIGN_RECORD_CODES
            .into_iter()
            .chain(GIVEN_UP_DESIGN_RECORD_CODES)
        {
            assert!(F3dLossCode::ALL.contains(&code));
            assert!(seen.insert(code.kind().local_code().to_owned()));
        }
    }

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = F3dLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "scene.override-refused",
                "settings.up-axis-unrecognized",
                "settings.up-axis-record-unreadable",
                "scene.visibility-native",
                "feature.draft-parting-controls-partial",
                "feature.draft-neutral-anchor-assumed",
                "feature.draft-slope-out-of-range",
                "feature.base-face-regions-native",
                "feature.base-face-source-carrier-unresolved",
                "feature.base-face-thickness-unresolved",
                "feature.base-face-material-side-unresolved",
                "feature.base-face-result-choice-unresolved",
                "feature.contour-vector-owner-distance-role-assumed",
                "feature.contour-profile-native",
                "feature.contour-default-side-one-left-assumed",
                "feature.contour-envelope-unresolved",
                "feature.contour-owner-vector-direction-assumed",
                "feature.contour-operation-four-body-assumed",
                "feature.contour-result-unresolved",
                "feature.contour-settings-rule-reference-assumed",
                "feature.contour-rule-first-radius-assumed",
                "feature.contour-material-scalar-factor-assumed",
                "feature.contour-inline-thickness-assumed",
                "feature.contour-radius-database-unit-assumed",
                "feature.contour-distance-database-unit-assumed",
                "feature.contour-cached-owner-distance-assumed",
                "feature.contour-single-distance-one-sided-assumed",
                "feature.contour-distance-unresolved",
                "feature.contour-first-radius-override-assumed",
                "feature.contour-direction-unresolved",
                "feature.contour-thickness-unresolved",
                "feature.contour-radius-unresolved",
                "feature.contour-rule-factor-unresolved",
                "feature.unfold-all-bends-flag-assumed",
                "feature.edge-flange-reference-envelope-unresolved",
                "feature.edge-flange-parameter-owner-backlink-assumed",
                "feature.edge-flange-cached-parameter-assumed",
                "feature.edge-flange-parameter-unit-assumed",
                "feature.edge-flange-parameter-unresolved",
                "feature.edge-flange-stored-radius-inside-assumed",
                "feature.edge-flange-radius-unresolved",
                "feature.edge-flange-selection-native",
                "feature.edge-flange-boundary-reference-native",
                "feature.edge-flange-boundary-scope-reference-assumed",
                "feature.edge-flange-full-width-assumed",
                "feature.edge-flange-signed-offsets-assumed",
                "feature.edge-flange-datum-unresolved",
                "feature.edge-flange-position-unresolved",
                "feature.edge-flange-width-unresolved",
                "feature.edge-flange-height-target-native",
                "feature.edge-flange-height-target-scope-assumed",
                "feature.unfold-control-unresolved",
                "feature.unfold-selection-slots-assumed",
                "feature.unfold-selection-native",
                "feature.unfold-envelope-unresolved",
                "feature.unfold-material-scalar-factor-assumed",
                "feature.unfold-rule-factor-unresolved",
                "feature.unfold-settings-rule-reference-assumed",
                "feature.refold-group-source-assumed",
                "feature.refold-source-unresolved",
                "feature.bend-line-owner-order-assumed",
                "feature.bend-stationary-role-assumed",
                "feature.bend-angle-unresolved",
                "feature.bend-radius-first-assumed",
                "feature.bend-stationary-selection-native",
                "feature.bend-rule-radius-unresolved",
                "feature.bend-angle-owner-backlink-assumed",
                "feature.bend-cached-owner-angle-assumed",
                "scene.override-ambiguous",
                "scene.override-unbound",
                "assembly.operation-table-pose",
                "assembly.operation-table-path-ordinal-assumed",
                "assembly.operation-table-version-assumed",
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
                "feature.cylinder-local-frame-assumed",
                "feature.sphere-prefix-framing-assumed",
                "feature.sphere-placement-flags-assumed",
                "feature.sphere-local-matrix-framing-assumed",
                "feature.sphere-local-frame-assumed",
                "feature.sphere-placement-frame-assumed",
                "feature.sphere-diameter-owner-assumed",
                "feature.sphere-diameter-source-assumed",
                "feature.tracking-path-untyped",
                "feature.construction-closing-flag-assumed",
                "sketch.curve-tail-marker-assumed",
                "sketch.curve-tail-construction-assumed",
                "feature.chamfer-edge-role-assumed",
                "feature.chamfer-fixed-face-group-assumed",
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
                "feature.coil-short-controls-assumed",
                "feature.coil-target-unresolved",
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
                "assembly.stored-frame-occurrence",
                "assembly.local-occurrence-owner-cycle",
                "assembly.local-occurrence-expansion-limit",
                "feature.occurrence-path-unresolved",
                "sketch.component-owner-unresolved",
                "sketch.name-composed-from-feature-ordinal",
                "sketch.membership-conflict",
                "sketch.folder-closing-reference",
                "sketch.folder-unprojected",
                "sketch.container-header-unrecognized",
                "sketch.geometry-group-member-unlisted",
                "sketch.geometry-group-member-unbacklinked",
                "assembly.component-insert-unplaced",
                "assembly.component-insert-order-unknown",
                "assembly.component-insert-container-assumed",
                "assembly.legacy-insert-frame-assumed",
                "assembly.component-definition-revised",
                "assembly.component-naming-space-unbound",
                "assembly.root-instance-definition-unbound",
                "assembly.local-occurrence-envelope-superseded",
                "assembly.local-occurrence-envelope-native",
                "assembly.occurrence-visibility-native",
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
                "assembly.joint-origin-envelope-matrix-discarded",
                "assembly.rigid-group-frame-unparsed",
                "assembly.joint-motion-undecoded",
                "assembly.joint-rest-unprojected",
                "assembly.joint-activation-assumed",
                "assembly.joint-angular-limits-native",
                "assembly.operand-path-pairs-native",
                "assembly.root-path-identities-native",
                "assembly.operand-descent-ambiguous",
                "assembly.operand-descent-unresolved",
                "assembly.member-joint-placement-bound",
                "assembly.joint-source-binding-assumed",
                "assembly.connector-operands-assumed",
                "assembly.motion-binding-assumed",
                "assembly.additional-owner-motion-assumed",
                "assembly.extended-tail-assumed",
                "assembly.extended-typed-tail-assumed",
                "assembly.operand-nested-path-retained",
                "assembly.operand-path-word-assumed",
                "drawing.document-omitted",
                "history.binding-budget-exceeded",
                "history.binding-work-exceeded",
                "feature.recipe-search-budget-exceeded",
                "feature.face-recipe-work-exceeded",
                "parameter.order-budget-exceeded",
                "history.record-framing-failed",
                "design.body-binding-unresolved",
                "design.history-only-body-omitted",
                "design.flagged-base-feature-body-selected",
                "appearance.reference-image-native",
                "appearance.reference-image-flag-assumed",
                "appearance.decal-native",
                "appearance.decal-mapping-assumed",
                "appearance.decal-placement-native",
                "appearance.decal-placement-layout-assumed",
                "appearance.decal-placement-normal-assumed",
                "appearance.decal-target-unresolved",
                "appearance.decal-image-unresolved",
                "appearance.decal-definition-unresolved",
                "appearance.body-override-precedence-assumed",
                "appearance.body-override-superseded",
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
                "feature.mesh-tessellate-scope-unparsed",
                "feature.mesh-tessellate-fields-uninterpreted",
                "feature.mesh-tessellate-source-unresolved",
                "feature.mesh-plane-cut-scope-unparsed",
                "feature.mesh-plane-cut-tail-partial",
                "feature.mesh-plane-cut-plane-partial",
                "feature.mesh-plane-cut-selector-unruled",
                "feature.mesh-plane-cut-fields-uninterpreted",
                "feature.mesh-plane-cut-source-unresolved",
                "feature.mesh-plane-cut-selection-unresolved",
                "feature.mesh-plane-cut-membership-assumed",
                "feature.mesh-plane-cut-matrix-orientation-assumed",
                "feature.mesh-plane-cut-units-assumed",
                "feature.mesh-plane-cut-model-space-assumed",
                "feature.mesh-plane-cut-retained-side-assumed",
                "feature.mesh-plane-cut-split-faces-assumed",
                "feature.mesh-plane-cut-uniform-fill-assumed",
                "feature.mesh-combine-scope-unparsed",
                "feature.mesh-combine-tail-partial",
                "feature.mesh-combine-selector-unruled",
                "feature.mesh-combine-fields-uninterpreted",
                "feature.mesh-combine-source-unresolved",
                "feature.mesh-combine-selection-unresolved",
                "feature.mesh-combine-merge-assumed",
                "feature.mesh-combine-legacy-boolean-assumed",
                "feature.mesh-combine-keep-tools-assumed-false",
                "feature.mesh-combine-destination-assumed-target-component",
                "feature.mesh-combine-keep-tools-unresolved",
                "feature.mesh-combine-destination-unresolved",
                "feature.mesh-conversion-tail-partial",
                "feature.mesh-conversion-method-unresolved",
                "feature.mesh-conversion-method-assumed",
                "feature.mesh-conversion-envelope-assumed",
                "feature.mesh-conversion-source-unresolved",
                "feature.mesh-conversion-output-unresolved",
                "feature.mesh-separate-scope-unparsed",
                "feature.mesh-separate-tail-partial",
                "feature.mesh-separate-reference-unresolved",
                "feature.mesh-separate-reference-mismatch",
                "feature.mesh-separate-selector-unruled",
                "feature.mesh-separate-shells-method-assumed",
                "feature.mesh-separate-face-groups-method-assumed",
                "feature.mesh-separate-all-groups-assumed",
                "feature.mesh-separate-selected-groups-assumed",
                "feature.mesh-separate-multiple-bodies-carrier-assumed",
                "feature.mesh-separate-multiple-bodies-polarity-assumed",
                "feature.mesh-separate-keep-body-carrier-assumed",
                "feature.mesh-separate-keep-body-polarity-assumed",
                "feature.mesh-separate-prefix-assumed",
                "feature.mesh-separate-output-role-assumed",
                "feature.mesh-separate-output-unresolved",
                "feature.mesh-separate-source-membership-assumed",
                "feature.mesh-separate-source-unresolved",
                "feature.mesh-separate-source-fields-uninterpreted",
                "feature.mesh-separate-reference-roles-assumed",
                "feature.mesh-separate-membership-unresolved",
                "feature.thread-live-face-unproven",
                "feature.thread-modeled-flag-assumed",
                "feature.thread-modeled-entries-absent",
                "feature.thread-construction-unresolved",
                "feature.thread-offset-native",
                "feature.thread-angle-native",
                "feature.thread-detail-native",
                "feature.thread-hole-native",
                "feature.thread-fractional-size-diameter-assumed",
                "feature.stored-bodies-empty",
                "feature.stored-bodies-recipe-absent",
                "feature.history-body-bounded",
                "feature.history-body-undecoded",
                "feature.history-body-consumed-withheld",
                "feature.body-history-incomplete",
                "feature.body-history-owner-unresolved",
                "feature.body-history-unread",
                "feature.owned-body-pairing-assumed",
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
                "feature.pattern-feature-seed-substituted",
                "feature.revolve-plane-work-plane-assumed",
                "feature.circular-pattern-placement-not-transferred",
                "feature.circular-pattern-suppression-not-transferred",
                "feature.circular-pattern-distribution-assumed",
                "feature.circular-pattern-controls-assumed",
                "feature.pattern-payload-seed-unconfirmed",
                "feature.rectangular-pattern-distance-form-assumed",
                "feature.rectangular-pattern-inactive-symmetric-dropped",
                "feature.rectangular-pattern-direction-one-record-assumed",
                "feature.rectangular-pattern-direction-unresolved",
                "feature.rectangular-pattern-direction-links-native",
                "feature.rectangular-pattern-construction-native",
                "feature.rectangular-pattern-design-native",
                "feature.rectangular-pattern-direction-record-disagrees",
                "feature.output-state-chain-unbound",
                "feature.output-body-unbound",
                "feature.pattern-copy-body-omitted",
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
                "feature.sheet-metal-rule-controls-native",
                "feature.sheet-metal-profile-native",
                "feature.base-flange-profile-unresolved",
                "feature.base-flange-thickness-unresolved",
                "feature.base-flange-side-unresolved",
                "feature.base-flange-side-assumed",
                "feature.scope-unprojected",
                "feature.move-unprojected",
                "feature.modify-unprojected",
                "feature.hole-extent-unproven",
                "feature.hole-placement-unproven",
                "feature.hole-placement-point-unselected",
                "feature.point-direction-inputs-unrepresented",
                "feature.hole-reference-dimensions-native",
                "feature.hole-parameters-independent-assumed",
                "feature.hole-shape-partial-assumed",
                "feature.hole-extent-leading-word-assumed",
                "feature.hole-tapped-diameter-stored",
                "feature.hole-thread-minor-diameter-assumed",
                "feature.hole-taper-tapped-bore-cylindrical",
                "feature.base-feature-null-flag-assumed",
                "feature.base-feature-suffix-run-order-assumed",
                "feature.fillet-padding-assumed",
                "feature.fillet-midpoint-order-assumed",
                "feature.fillet-set-input-unresolved",
                "feature.fillet-set-group-unresolved",
                "feature.hole-standard-sizing-assumed",
                "feature.hole-clearance-fields-assumed",
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
                "parameter.header-flag-uninterpreted",
                "parameter.driven-flag-assumed",
                "parameter.owner-trailer-flag-uninterpreted",
                "parameter.owner-unstored",
                "parameter.owner-unrecognized",
                "parameter.owner-unparsed",
                "parameter.unit-untyped",
                "material.distance-unit-untyped",
                "material.face-appearance-ambiguous",
                "parameter.expression-unbound",
                "parameter.referenced-design-unscoped",
                "parameter.owner-outside-timeline",
                "parameter.owner-unresolved-omitted",
                "parameter.name-blank",
                "configuration.referenced-design-unscoped",
                "configuration.override-unstated",
                "configuration.override-value-unprojected",
                "configuration.scope-without-active-row",
                "configuration.property-unprojected",
                "history.rollback-unread",
                "history.rollback-bound-unread",
                "feature.coordinate-system-construction-native",
                "feature.coordinate-system-construction-dropped",
                "feature.coordinate-system-frame-unresolved",
                "history.dependency-unprojected",
                "history.dependency-ambiguous",
                "history.face-selection-unbound",
                "history.delete-face-member-order-assumed",
                "history.edge-midpoints-unresolved",
                "history.fillet-edge-inferred",
                "history.edge-selection-inferred",
                "history.edge-selection-deleted-set-assumed",
                "history.edge-selection-leftover-assumed",
                "history.edge-selection-first-side-pair-assumed",
                "history.edge-continuity-assumed-sharp",
                "history.edge-history-inferred",
                "history.output-blob-history-inferred",
                "history.edge-recipe-history-inferred",
                "history.body-recipe-history-inferred",
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
                "sketch.curve-undecoded",
                "sketch.curve-record-undecoded",
                "sketch.surface-undecoded",
                "sketch.text-undecoded",
                "sketch.relation-undecoded",
                "sketch.folder-undecoded",
                "sketch.ordinal-collection-undecoded",
                "sketch.container-undecoded",
                "sketch.geometry-group-undecoded",
                "feature.timeline-undecoded",
                "design.primary-record-undecoded",
                "configuration.projection-native",
                "sketch.owner-conflict",
                "sketch.relation-owner-unresolved",
                "configuration.document-undecoded",
                "feature.bend-settings-undecoded",
                "design.body-bounds-undecoded",
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
                "feature.extrude-sketch-text-skipped",
                "feature.extrude-profile-group-union-assumed",
                "feature.extrude-profile-sketch-absent",
                "feature.extrude-surface-profile-assumed",
                "feature.extrude-symmetric-distance-assumed",
                "feature.extrude-wall-thickness-native",
                "feature.extrude-auxiliary-selection-native",
                "feature.extrude-face-control-assumed",
                "feature.extrude-sketch-placement-native",
                "feature.extrude-target-option-assumed",
                "feature.extrude-opaque-distance-word-assumed",
                "feature.extrude-start-face-absent-assumed",
                "feature.extrude-start-face-unresolved",
                "feature.extrude-zero-side-taper-ignored",
                "feature.extrude-profile-face-order-assumed",
                "feature.extrude-profile-face-search-capped",
                "feature.extrude-profile-cap-faces-inferred",
                "feature.extrude-profile-deleted-faces-inferred",
                "feature.extrude-profile-face-set-disputed",
                "feature.extrude-profile-face-curved",
                "feature.sketch-profile-flags-assumed",
                "feature.unevaluated-state-assumed",
                "feature.no-result-active-assumed",
                "feature.state-byte-above-one",
                "feature.profile-arrangement-capped",
                "feature.profile-join-envelope-capped",
                "feature.profile-crossing-runs-capped",
                "feature.profile-densification-capped",
                "feature.profile-crossing-loop-capped",
                "feature.profile-crossing-loop-assumed",
                "feature.profile-coincident-direction-assumed",
                "feature.profile-coincident-boundary-assumed",
                "feature.profile-crossing-runs-assumed",
                "feature.profile-partial-entry-assumed",
                "feature.profile-loop-flags-assumed",
                "feature.profile-cut-regions-assumed",
                "feature.profile-region-identities-stale",
                "feature.profile-member-reference-lost",
                "feature.profile-text-glyphs-assumed",
                "feature.profile-text-glyphs-native",
                "feature.profile-island-absence-assumed",
                "feature.profile-piece-partition-assumed",
                "feature.profile-polyline-side-assumed",
                "feature.profile-cap-outline-assumed",
                "feature.profile-cap-carrier-assumed",
                "feature.profile-hole-boundary-inferred",
                "feature.profile-region-hole-inferred",
                "feature.profile-body-circle-rings-assumed",
                "feature.spatial-profile-transition-assumed",
                "feature.path-selection-native",
                "feature.face-selection-native",
                "feature.delete-face-frame-controls-assumed",
                "feature.delete-face-group-selection-assumed",
                "feature.surface-delete-face-wide-prologue-assumed",
                "feature.face-selection-active-substituted",
                "feature.surface-offset-group-ordinals-assumed",
                "feature.form-source-unavailable",
                "feature.form-definition-unavailable",
                "feature.form-membership-unresolved",
                "feature.form-legacy-serializer-membership-assumed",
                "feature.form-legacy-serializer-order-assumed",
                "feature.form-cage-binding-ambiguous",
                "feature.form-body-references-partial-assumed",
                "feature.form-body-registration-unsupported",
                "feature.form-reference-type-conflict",
                "feature.form-foreign-reference-unresolved",
                "feature.form-tracking-owner-conflict",
                "feature.form-tracking-run-unresolved",
                "feature.form-tracking-fixed-slots-assumed",
                "feature.form-serializer-reference-unresolved",
                "feature.form-membership-conflict",
                "feature.scale-frame-length-form-assumed",
                "feature.scale-matrix-word-assumed",
                "feature.scale-unlisted-groups-assumed",
                "feature.work-axis-rule-unassigned",
                "feature.work-axis-endpoint-tolerance-assumed",
                "feature.work-axis-nonzero-parameters-solved-assumed",
                "feature.work-axis-partial-carrier-solved-assumed",
                "feature.work-axis-two-point-operands-unresolved-solved-assumed",
                "feature.work-axis-two-point-conflict-solved-assumed",
                "feature.work-axis-geometry-unrepresentable",
                "feature.work-axis-binding-unresolved",
                "feature.work-axis-leading-block-layout-assumed",
                "feature.work-axis-unsupported-version-legacy-layout-assumed",
                "feature.work-axis-legacy-layout-assumed",
                "feature.work-axis-candidates-unresolved",
                "feature.work-axis-input-null",
                "feature.work-axis-partial-line-native",
                "feature.body-selection-native",
                "feature.generated-body-history-unresolved",
                "feature.reverse-normal-body-unverified",
                "feature.reverse-normal-body-producer-unresolved",
                "feature.reverse-normal-link-unplaced",
                "feature.reverse-normal-link-shared",
                "feature.reverse-normal-link-order-assumed",
                "feature.combine-body-producer-unresolved",
                "feature.combine-operand-writer-assumed",
                "feature.combine-tool-external-final-state-assumed",
                "feature.silhouette-split-prefix-unparsed",
                "feature.silhouette-split-mode-unresolved",
                "feature.silhouette-split-direction-unresolved",
                "feature.silhouette-split-target-unresolved",
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
                "feature.replace-face-frame-assumed",
                "feature.replace-face-groups-aggregated-assumed",
                "feature.replace-face-overlap-unresolved",
                "feature.replace-face-selection-assumed",
                "feature.replace-face-tangent-chain-assumed",
                "feature.replace-face-support-assumed",
                "feature.replace-face-identity-assumed",
                "feature.replace-face-flagged-prefix-word-assumed",
                "feature.surface-untrim-frame-assumed",
                "feature.surface-untrim-group-selection-assumed",
                "feature.surface-untrim-stored-loop-groups-assumed",
                "feature.surface-untrim-loop-groups-aggregated-assumed",
                "feature.surface-untrim-extension-unresolved",
                "feature.surface-untrim-extension-unrepresentable",
                "feature.surface-untrim-negative-extension-assumed",
                "feature.surface-untrim-padding-assumed",
                "feature.surface-untrim-trailing-identity-assumed",
                "feature.unstitch-frame-assumed",
                "feature.unstitch-group-selection-assumed",
                "feature.unstitch-prologue-controls-assumed",
                "feature.combine-selection-native",
                "feature.combine-definition-unresolved",
                "feature.combine-middle-list-omitted-assumed",
                "feature.combine-component-destination-assumed",
                "feature.combine-named-tail-prologue-assumed",
                "feature.combine-tool-body-unbound",
                "feature.combine-tool-external-body-unbound",
                "feature.combine-tool-result-state-unbound",
                "feature.combine-operand-body-unbound",
                "feature.combine-tool-recipe-replacement-assumed",
                "feature.combine-prologue-assumed",
                "feature.combine-operation-entries-assumed",
                "feature.combine-kept-tools-cut-assumed",
                "feature.combine-operand-list-refused",
                "feature.combine-tool-consumption-assumed",
                "feature.split-face-method-unresolved",
                "feature.split-face-direction-unresolved",
                "feature.split-face-groups-unresolved",
                "feature.split-face-selection-unresolved",
                "feature.split-face-selection-projection-failed",
                "feature.split-face-carrier-type-assumed",
                "feature.split-face-indexed-operand-assumed",
                "feature.fixed-fillet-assignment-native",
                "feature.fillet-radius-unresolved",
                "feature.fillet-references-unresolved",
                "feature.fillet-full-round-center-unresolved",
                "feature.fillet-set-list-native",
                "feature.extrude-extent-native",
                "feature.extrude-termination-missing",
                "feature.revolve-layout-assumed",
                "feature.revolve-frame-unmatched",
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
                "feature.loft-solid-inferred",
                "feature.surface-extend-method-unresolved",
                "feature.surface-extend-edges-as-faces",
                "feature.split-tool-extension-assumed",
                "feature.split-tool-extension-unresolved",
                "feature.split-body-face-tool-group-assumed",
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
                "feature.shell-removed-faces-assumed",
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
                "feature.boundary-fill-cells-native",
                "feature.boundary-fill-cells-unresolved",
                "feature.boundary-fill-operation-unresolved",
                "feature.boundary-fill-tools-unresolved",
                "feature.boundary-fill-targets-unresolved",
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
                "mesh.component-flag-uninterpreted",
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
                "xref.lineage-disagrees",
                "xref.identity-blank",
                "xref.archive-identity-disagrees",
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
                "assembly.local-occurrence-marked-prelude-assumed",
                "assembly.local-occurrence-object-prelude-assumed",
                "assembly.placement-children-differ",
                "assembly.instance-reference-shared",
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
                "design.empty",
                "design.mesh-only-untransferred",
                "assembly.components-external",
                "geometry.nurbs-surface-carrier",
                "geometry.sketch-region-body-omitted",
                "geometry.nurbs-curve-carrier",
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
                "topology.face-boundary-unstated",
                "container.missing-geometry-stream",
                "xref.cycle",
                "xref.member-missing",
                "xref.member-undecoded",
                "tspline.record-untyped",
                "tspline.radial-map-unresolved",
                "tspline.cage-undecoded",
                "source.dialect-unverified",
                "writer.appearance-properties-omitted",
                "writer.procedural-reduced",
                "writer.pcurve-omitted",
                "writer.subd-omitted",
                "writer.asset-omitted",
                "writer.joint-omitted",
                "writer.parameter-omitted",
                "writer.sketch-content-omitted",
                "writer.sketch-support-substituted",
                "writer.configuration-omitted",
                "writer.tessellation-omitted",
                "writer.source-association-omitted",
                "writer.body-omitted",
                "writer.face-omitted",
                "writer.geometry-omitted",
                "writer.edge-range-assumed",
                "writer.rollback-assumed",
                "writer.appearance-omitted",
                "writer.history-unrestored",
                "writer.appearance-guid-allocated",
                "source.kernel-dialect-unverified",
                "source.kernel-carrier-unparseable",
                "source.dialect-layer-collision",
                "brep.text-header-noncanonical",
                "brep.header-tolerance-unresolved",
                "brep.header-metadata-unresolved",
                "source.manifest-metadata-unresolved",
                "source.unindexed-records-retained",
                "design.intent-undecoded",
                "design.body-display-name-ambiguous",
                "work-plane.display-name-ambiguous",
                "work-plane.display-name-undecoded",
                "writer.integer-as-real",
                "writer.design-record-omitted",
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
