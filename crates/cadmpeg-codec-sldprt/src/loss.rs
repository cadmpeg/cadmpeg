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
use cadmpeg_ir::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

/// A stable, machine-readable identifier for one `.sldprt` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`SldprtLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SldprtLossCode {
    /// Part root scalar prefix has no supported class-specific reading.
    MetadataPartRootUnsupported,
    /// Configuration-manager class has no proven scalar roles or serializer.
    MetadataConfigurationManagerOpaque,
    /// Default-property carrier stores its properties in a form the decoder
    /// does not read.
    MetadataDefaultCarrierUnsupported,
    /// Default properties have multiple physical namespace owners.
    MetadataDefaultOwnerAmbiguous,
    /// Saved alias or value identifiers are not unique.
    MetadataDefaultAliasAmbiguous,
    /// Saved property has no represented usable scalar value.
    MetadataDefaultValueUnsupported,
    /// Saved configuration mass properties cannot be compared to active geometry.
    MetadataMassPropertiesNotCompared,
    /// Configuration-property namespace has multiple physical owners.
    MetadataConfigurationPropertyOwnerAmbiguous,
    /// Configuration-property carrier stores its properties in a form the
    /// decoder does not read.
    MetadataConfigurationPropertyCarrierUnsupported,
    /// Configuration-property record has no supported literal scalar fact.
    MetadataConfigurationPropertyValueUnsupported,

    /// Active configuration identity does not resolve to exactly one record.
    ConfigActiveIdentityUnresolved,
    /// Active configuration does not resolve to the active geometry partition.
    ConfigActivePartitionMismatch,
    /// The active partition is not chosen: a section the choice needs was
    /// given up.
    ConfigActivePartitionUnread,
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
    /// The configuration pass reached its work cap; later configurations
    /// keep empty projected states.
    ConfigProjectionWorkCapped,
    /// A configuration's stored state has no design evaluation.
    ConfigStateUnstated,
    /// A configuration that is not active has no stored partition.
    ConfigStateNotStored,
    /// The appearances or tessellations on the records of a state that is not
    /// current are not stated.
    ConfigStatePresentationUnstated,
    /// The imported-feature body streams of a configuration that is not
    /// active are not taken into its state.
    ConfigStateImportedBodiesNotTransferred,
    /// A feature output on a body of a state that is not current, whose
    /// feature no face name of that body in the state's evaluation names, is
    /// not stated.
    ConfigStateOutputsUnstated,
    /// Parameter lacks an evaluated scalar or a regenerable finite value.
    ParameterUnevaluated,
    /// Parameter records have empty, duplicate, or colliding names/ordinals.
    ParameterAmbiguousIdentity,
    /// Semantic dimension record is unbound or retains a native subtype.
    PmiDimensionUnbound,
    /// A `PMISemanticDataDB` section reached its parse work cap; later
    /// candidate maps were not read.
    PmiSemanticParseCapped,
    /// SWIFT annotation class has no format-neutral PMI representation.
    PmiSwiftAnnotationUnsupported,
    /// A SWIFT feature walk met a feature only past its depth or work bound;
    /// the targets or nominal values it feeds stay unresolved.
    PmiSwiftFeatureWalkCapped,
    /// A SWIFT diameter has an omitted nominal and no resolved implicit value.
    PmiSwiftImplicitNominalUnresolved,
    /// A SWIFT position retains a source `ShapeAspect` when its target is unresolved.
    PmiSwiftPositionTargetUnresolved,
    /// Counted spherical-diameter text selects a second sphere by equal radius.
    PmiSwiftSphericalDiameterTargetCountTextEqualRadiusAssumed,
    /// A counted spherical-diameter display has no unique resolved sphere target set.
    PmiSwiftSphericalDiameterTargetUnresolved,
    /// A SWIFT geometric-tolerance display has no established semantic annotation.
    PmiSwiftPresentationUnresolved,
    /// Internal flatness rows use an inferred characteristic convention.
    PmiSwiftInternalFlatnessSymbolAssumed,
    /// A SWIFT presentation archive declares displays that its framing discards.
    PmiSwiftPresentationUnframed,
    /// A SWIFT display selects faces through a one- or two-string in-place document.
    PmiSwiftFaceDocumentLayoutAssumed,
    /// A SWIFT display selects faces through an in-place document whose tail length the file's `swVersion` does not establish.
    PmiSwiftFaceDocumentTailAssumed,
    /// Feature history record has duplicate identity or unresolved references.
    HistoryIncompleteReferences,
    /// The stored marks do not fix the rollback position of the feature tree.
    HistoryRollbackUnread,
    /// A configuration stores states between features that its states
    /// between features do not state.
    HistoryStatesUnread,
    /// The `moPMarkRecord_c` class index of a configuration is the class
    /// token whose records name the most main-line marks.
    HistoryRollbackMarkClassInferred,
    /// A Keywords root holds an `Application` element, which the native history
    /// keeps and no neutral record holds.
    HistoryApplicationNotTransferred,
    /// An `Application` element nests deeper than the levels the native history
    /// keeps, so the elements below them are not kept.
    HistoryApplicationNestingCut,
    /// A Keywords text holds comments, processing instructions, text with
    /// edge whitespace or namespace prefixes, which the native history does
    /// not keep.
    HistoryXmlNormalised,
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
    /// A sketch line, circle, or arc handle keeps its marker role because no
    /// geometry was admitted for it.
    SketchHandleUnresolved,
    /// A sketch line handle keeps its marker role because its two read
    /// endpoints coincide.
    SketchLineDegenerate,
    /// A sketch line keeps the line its marker reading gives although its
    /// `sgSketch` curve record states an arc or another form that gives no
    /// entity.
    SketchArcRecordConflict,
    /// A sketch marker that the marker layouts skip and only its `sgSketch`
    /// record admits gives no entity, because that record gives none in the
    /// sketch.
    SketchMarkerRecordUnread,
    /// A profile-stream sketch that no feature binds repeats the profile
    /// loops of the marker sketch that a sketch feature binds.
    SketchSecondReading,
    /// The marker sketch of a sketch feature stays unbound beside the
    /// profile-stream circle bound to the feature.
    SketchMarkerReadingUnbound,
    /// A relation point marker whose record form reads a coordinate pair
    /// that holds a nonzero subnormal value gives no relation point.
    SketchPointCoordinatesSubnormal,
    /// A relation point marker gives no relation point where the decode
    /// does not settle its place: no unique fitted marker transform and no
    /// sketch frame place it; or, for a marker that only a list-read record
    /// reaches, the fitted transform and the frame place it more than the
    /// linear resolution apart and a curve end or centre of the sketch or a
    /// vertex or circular-edge centre of the solid marks both places or
    /// neither, or the sketch its feature binds is read from another
    /// feature-input lane.
    SketchRelationPointPlaceUnsettled,
    /// A profile sketch lies on a plane the decoder assumes (open item
    /// DI-73): the plane that the preceding object's or the lane's plane
    /// reference names, the Top plane its feature-input reference names, the
    /// only frame of its legacy configuration lane, or the Front lane fallback
    /// frame (SF-01).
    SketchPlaneBindingAssumed,
    /// A geometry-locus curve uses assumed zero-based sketch point-record indices.
    SketchPointRecordIndexBasisAssumed,
    /// A sketch handle takes the arc of its `sgSketch` curve record although
    /// the arc, placed in the sketch, ends off its endpoint point records.
    SketchArcRecordOffChord,
    /// A compact sketch profile failed neutral profile admission.
    SketchProfileRejected,
    /// A list line with profile role 1 is marked construction from selector bit 3 (DI-77).
    SketchListedLineConstructionAssumed,
    /// A framed list curve drops a constraint-owning reading of +17 (DI-77).
    SketchListedCurveRelationDropped,
    /// Native sketch relation has no projected neutral constraint.
    SketchRelationUnprojected,
    /// Native sketch relation is claimed by multiple neutral objects.
    SketchRelationMultiplyProjected,
    /// A sketch dimension of a compound-file lane that no relation-class
    /// declaration claims has an unknown relation class and forms no
    /// relation instance (open item EV-05).
    SketchDimensionClassUnknown,
    /// A sketch dimension of a compound-file lane whose relation-class token
    /// gives it the family and the two sketch points of a kept relation
    /// forms no second relation instance (open item EV-05).
    SketchDimensionRepeated,
    /// Feature retains its native kind without a complete neutral operation.
    FeatureNativeKindRetained,
    /// Native feature-input operation object does not bind uniquely to a feature.
    FeatureInputObjectUnbound,
    /// A solid-body transmit remains in its native feature-input or stock-body section.
    FeatureInputSolidBodyRetained,
    /// The archive walk of a feature-input lane stopped before the stream's
    /// end, so the lane scalars keep no archive parameter class.
    FeatureInputArchiveWalkStopped,
    /// A fillet or chamfer names its edges by the feature-local component paths
    /// of the feature that made them; the sketch entities that generate those
    /// edges are not established, so the selection keeps the paths.
    FeatureBlendEdgeGeneratorUnstated,
    /// Every edge vector of a fillet or chamfer lies inside a
    /// `moPMarkRecord_c`; the selection takes the record's edge references as
    /// the feature's edge list.
    FeatureBlendMarkReferencesAssumed,
    /// Typed feature retains native or unresolved required operation operands.
    FeatureTypedOperandIncomplete,
    /// Sweep path geometry keeps its native reference after an unreadable record.
    FeatureSweepPathRetained,
    /// Unframed spatial sweep path lines with an identifier trailer use their
    /// primary-point roster.
    FeatureSweepPathRosterAssumed,
    /// A sweep path component source follows a u32 header word.
    FeatureSweepPathHeaderAssumed,
    /// A generated solid sweep uses the Merge reading of its Boolean operation.
    FeatureSweepMergeAssumed,
    /// Extrusion profile curve data remains unresolved.
    FeatureProfileCurvesUnavailable,
    /// Extrusion travel sense remains in its native feature-input lane.
    FeatureExtrudeDirectionUnresolved,
    /// Extrusion keeps the travel its solid shows against the reversal flag its
    /// record states.
    FeatureExtrudeReversalContradicted,
    /// Extrusion travels against its profile normal on the reversal flag its
    /// record states alone: no solid that the decoder tests holds or contradicts that
    /// travel.
    FeatureExtrudeReversalAssumed,
    /// Extrusion start condition is not the sketch plane and has no supported reading.
    FeatureExtrudeStartUnresolved,
    /// An offset reference plane keeps the side its `D1` sign gives: `D1` is a
    /// magnitude, and neither a stored frame of the object nor the point and
    /// normal of its plane-data object states the side.
    FeatureOffsetPlaneSideAssumed,
    /// An offset reference plane stores a frame, or a point and normal, that
    /// is not the plane the decoder places from its source plane and `D1`.
    FeatureOffsetPlaneFrameConflict,
    /// An offset reference plane takes its distance from the `D1` display: the
    /// decoder reads no value of its `D1` parameter object that the display
    /// shows.
    FeatureOffsetPlaneDistanceDisplayed,
    /// A revolution keeps a stored axis that lies outside the plane the
    /// decoder binds its profile sketch to (DI-85).
    FeatureRevolveAxisOffProfilePlane,
    /// A revolution reads its axis from the tail of an `moLineRef_w` record
    /// whose words before the scalar record are not read, or keeps no axis
    /// from such records (DI-86).
    FeatureRevolveAxisRecordPrefixUnread,
    /// Thin extrusion retains an ambiguous inline Boolean operation.
    FeatureThinOperationAmbiguous,
    /// Native edge selection has a refused terminal feature owner.
    FeatureEdgeTerminalUnresolved,
    /// A compact body-path boundary has competing local-ID and tagged-entry spellings.
    FeatureBodyPathAmbiguous,
    /// Body delete/keep feature retains native selection without a decoded mode.
    FeatureBodyRetentionUnresolved,
    /// Face or procedural construction references an untyped support surface.
    GeometryFaceSupportSurfaceUntyped,
    /// Different surface carrier categories claim one node index.
    GeometrySurfaceCarrierAmbiguous,
    /// Offset validity does not establish an exact construction.
    GeometryOffsetValidityUnresolved,
    /// Stored spun-profile bounds are ignored and the whole profile is revolved.
    GeometrySpunProfileIntervalIgnored,
    /// A rolling-ball blend face has no solved cache; it keeps its construction.
    GeometryBlendCacheUnsolved,
    /// Edge references an untyped support curve carried opaque.
    GeometryEdgeSupportCurveUntyped,
    /// Tolerant edge has no curve because its fin geometry is not read.
    GeometryTolerantEdgeCurveUnread,
    /// Referenced POINT records have conflicting complete frames.
    GeometryPointRecordAmbiguous,
    /// Compact curve records of one attribute disagree on orientation, so an
    /// edge on that curve has no range.
    GeometryCurveMarkerConflict,
    /// A derived pcurve parameter has multiple geometric candidates.
    GeometryPcurveAmbiguous,
    /// A NURBS surface solver exhausted its local work cap; the derived pcurve is skipped.
    GeometrySurfaceSolverWorkCapped,
    /// A spline carrier's pole and weight lanes do not pair.
    GeometrySplineLanesUnpaired,
    /// A spline carrier stores a control-point weight that is zero or
    /// negative.
    GeometrySplineWeightNonpositive,
    /// A spline carrier's dimension, counts, or multiplicities contradict
    /// each other or its arrays.
    GeometrySplineFieldsInconsistent,
    /// A CONE node stores a negative sine or cosine of its half-angle, whose
    /// surface is unresolved (`GC-49`).
    GeometryConeHalfAngleSignUnresolved,
    /// A CONE node's stored sine and cosine break the unit identity.
    GeometryConeHalfAngleIdentityRefused,
    /// The stream's INTERSECTION declaration states no node layout that
    /// frames, so its intersection curves are not read.
    GeometryIntersectionLayoutUnknown,
    /// A neutral carrier uses an approximate intersection or fitted cache.
    GeometryCarrierApproximated,
    /// Current face records carry conflicting or incoherent color bindings.
    AppearanceFaceColorUnresolved,
    /// Appearance assignments have no unambiguous `DisplayLists` target.
    AppearanceAssignmentUnresolved,
    /// Native appearance ownership, state, or precedence has no unique scope.
    NativeAppearanceScopeUnresolved,
    /// `DisplayLists` tessellation does not resolve to its B-rep face owners.
    TessellationFaceOwnershipUnresolved,
    /// Surface-owner assignment ran out of its local work cap.
    TessellationOwnerWorkCapped,
    /// Display-face tables carry conflicting reused class tags, so the
    /// persistent join is withheld.
    TessellationReusedTagAmbiguous,
    /// `DisplayLists` meshes transferred without B-rep geometry ownership.
    TessellationDisplayWithoutGeometry,
    /// No body record links up with the faces; a body hierarchy was derived.
    TopologyBodyHierarchyDerived,
    /// A face keeps the loops before a missing loop or a loop another face owns.
    TopologyLoopChainCut,
    /// Current typed ownership nodes are absent from the decoded topology.
    TopologyOwnershipWithheld,
    /// The region has both proven and unresolved shell roles.
    TopologyShellRoleUnstated,
    /// Referenced FIN records have conflicting complete frames.
    TopologyFinRecordAmbiguous,
    /// A canonical edge whose FIN record is an unwalked boundary dummy lacks
    /// reciprocal partner corroboration of both ring-order endpoints, or a
    /// mutual twin FIN contradicts the ring-order endpoint.
    TopologyFinPartnerUnresolved,
    /// Referenced LOOP records have conflicting complete frames.
    TopologyLoopRecordAmbiguous,
    /// Referenced VERTEX records lack a complete owner-verified frame or IR-supported tolerance.
    TopologyVertexRecordUnresolved,
    /// A NURBS edge's vertex range is off its bound surface; no pcurve is derived.
    TopologyPcurveCarrierOffSurface,
    /// Parasolid B-rep geometry was not transferred (no resolved stream).
    GeometryParasolidNotTransferred,
    /// Mesh bodies whose facet geometry remains unresolved.
    GeometryMeshBodyNotTransferred,
    /// Imported-feature body streams of the active configuration that do not
    /// decode or that the decode does not take.
    GeometryImportedBodiesNotTransferred,
    /// A body's stored `res_linear` states no linear precision
    /// (`parasolid.md` §4).
    BodyResolutionInvalid,
    /// A body whose BODY node does not read states no `res_linear`, and the
    /// part tolerance does not count it (`parasolid.md` §4).
    BodyResolutionUnread,
    /// B-rep topology graph was not built for this file.
    TopologyGraphNotTransferred,
    /// Materials, tessellation, and metadata were not transferred.
    MaterialMetadataNotTransferred,
    /// No Parasolid partition/deltas stream was located in the container.
    ContainerNoParasolidStream,
    /// The legacy Parasolid zlib probe stopped at its work limit in a section.
    ContainerParasolidProbeCapped,
    /// The compound document admits a noncanonical CFB encoding that every
    /// stream it names is read past.
    ContainerNoncanonical,
    /// Preserved source image required for a byte-exact write was unavailable.
    SourcePreservedImageUnavailable,
    /// The document carries no usable `swVersion`, so no declared identity was
    /// verified.
    ///
    /// Charged exactly when the primary-layer [`crate::dialect`] match is
    /// `Admission::Residual`, from the same predicate that decides the
    /// admission. A residual `unknown` row is the absence of a declared
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
    /// Export omits records named by the counted loss message.
    WriterShellWireTopologyOmitted,
    /// Export omits records named by the counted loss message.
    WriterCoedgePcurveUsesOmitted,
    /// Export omits records named by the counted loss message.
    WriterSphereSeamCurvesOmitted,
    /// Export omits records named by the counted loss message.
    WriterAppearanceBindingsOmitted,
    /// Export omits records named by the counted loss message.
    WriterAttributesOmitted,
    /// Export omits the driven (reference) dimensions of a generated sketch.
    WriterDrivenDimensionsOmitted,
    /// Export omits records named by the counted loss message.
    WriterUnboundAppearancesOmitted,
    /// Export writes no `SWObjects` material for the appearances or bodies
    /// named by the counted loss message.
    WriterMaterialsOmitted,
    /// Export omits records named by the counted loss message.
    WriterTessellationMetadataOmitted,
    /// Semantic regeneration omits native cache or directory-padding bytes.
    WriterNativeEnvelopeAuxiliaryOmitted,
    /// Export writes the unit code in a generated `Units` member that no
    /// `SolidWorks` file is known to hold.
    WriterUnitsMemberGenerated,
    /// Export writes the generated feature history as a cadmpeg summary stream; the native
    /// feature tree holds none of the source's features.
    WriterNativeHistorySummary,
    /// Export writes as zero bytes the native skeleton's byte runs whose fields `sldprt.md`
    /// does not lay out.
    WriterNativeSkeletonUnlaidRuns,
    /// Export writes a generated part without the native skeleton.
    WriterNativeSkeletonOmitted,
    /// Export leaves out the named native streams of the generated part.
    WriterNativeStreamsOmitted,
    /// Export writes generated sketch marker records with no owning native object.
    WriterNativeSketchLaneOrphan,
    /// Export writes a native extrusion with no face references and a zero bounding box.
    WriterNativeBodyIdentityOmitted,
    /// Export generates partitions with no producer or persistent-id
    /// attribute, so feature outputs and face and body names are not written.
    WriterProducerAttributesOmitted,
    /// Export writes each native sketch with its profile body, which native top-level sketches
    /// do not hold.
    WriterNativeSketchProfileBodyKept,
    /// Export writes the named native sketches of one circle about the origin with a
    /// coincidence of the circle's centre and the origin point that the document does not state.
    WriterNativeSketchOriginRelationAdded,
    /// Export writes the named configurations with a name or partition index it assigned.
    WriterConfigurationIdentityAssigned,
    /// Export omits the named bodies.
    WriterBodiesOmitted,
    /// Export writes the named faces without their name.
    WriterFaceNamesOmitted,
    /// Export writes the named bodies without their name.
    WriterBodyNamesOmitted,
    /// Export writes the named appearances with their name and base colour only.
    WriterAppearanceFieldsOmitted,
    /// Export omits the named subdivision surfaces.
    WriterSubdOmitted,
    /// Export writes the named configuration active, where the document states none or several.
    WriterActiveConfigurationAssumed,
    /// Export writes no history record for the named design records.
    WriterFeaturesOmitted,
    /// Export writes no history record for the named features, whose result
    /// body the output does not hold, nor for the records that need them.
    WriterFeatureResultAbsent,
    /// Export writes the sketches without the named constraints.
    WriterSketchConstraintsOmitted,
    /// Export bounds the named edges by their vertices, without their range.
    WriterEdgeRangesOmitted,
    /// Export omits the named PMI annotations.
    WriterPmiOmitted,
    /// Export writes the named parameters with their displayed value, without
    /// the expression that drives it.
    WriterParameterExpressionsOmitted,
    /// Export writes the named approximation caches as exact carriers, without
    /// the fit tolerance they state.
    WriterCacheTolerancesOmitted,
    /// Export writes the named faces with a loop whose coedges do not meet end
    /// to end, which the ring order cannot state.
    WriterFaceLoopsUnchained,
    /// Export writes the named feature selections by their native selection
    /// token, without the input-topology identities they state.
    WriterSelectionIdentitiesOmitted,
    /// Export writes the named features without a construction field their
    /// SLDPRT record has no field for.
    WriterFeatureFieldsOmitted,
    /// Export writes the named parameters under the positional dimension name
    /// from which the SLDPRT record's construction value is read.
    WriterParametersRenamed,
    /// Export writes the named Keywords records with U+FFFD in place of each
    /// character XML 1.0 does not allow.
    WriterKeywordsCharactersReplaced,
    /// Export writes the text items and dimension values of the named
    /// Keywords records without their edge whitespace, and leaves out text
    /// items that hold only whitespace, which the reader does not keep.
    WriterKeywordsTextTrimmed,
    /// Export writes the named features as Keywords history records without
    /// the feature objects from which `SolidWorks` rebuilds a part.
    WriterHistoryKeywordsOnly,
    /// Export writes the named Keywords features under the `Feature` tag, or
    /// leaves out the named Keywords elements with their subtrees, because XML
    /// cannot hold the tag or the reader reads it as another record.
    WriterHistoryNameNotWritten,
    /// A regenerated Keywords text cannot be built or is refused by the
    /// reader, so the source member, or the retained source blocks of the
    /// member, are written in its place, or no member is written.
    WriterHistoryRegenerationRefused,
    /// A construction edit that shapes the solid is written as edited, and the
    /// output keeps the stored solid, which shows the state before the edit.
    /// Without the retained source image, it names the kinds of record that
    /// differ from the decoded source, whose edit may not shape the solid.
    WriterSolidStaleAfterEdit,
    /// An edit of the active configuration snapshot that the decoder
    /// synthesized from the model is not written; the written configuration
    /// states the model's state.
    WriterConfigurationSnapshotEditUnwritten,
    /// A variant edit of a feature is written in the record shape of the
    /// edited variant, which adds or drops a parameter or drops a source
    /// property of the feature.
    WriterFeatureRecordReshaped,
    /// The written part, decoded again, differs from the document as the
    /// write of the unedited source does: a writer defect no edit causes.
    WriterReadBackDiffers,
    /// The written part does not decode again or does not serialize, so it
    /// is not compared with the document.
    WriterReadBackUnread,
    /// An edit cleared a decoded extrusion reversal whose end-spec flag the
    /// writer found no place to clear.
    WriterExtrudeReversalUnwritten,
    /// An edit cleared a decoded extrusion reversal, and the writer cleared
    /// the flag in a lane outside the active configuration.
    WriterExtrudeReversalConfigurationCleared,
    /// A source record states a property whose key holds no non-whitespace
    /// character, so the property cannot be keyed and is not transferred.
    SourcePropertyKeyBlank,
    /// An assembly document carries no readable component tree, so no
    /// occurrence, placement, or component reference is transferred.
    AssemblyStructureNotTransferred,
    /// Assembly mate records are not transferred.
    AssemblyMatesNotTransferred,
    /// Assembly occurrences reference component documents that the decoder
    /// does not open.
    AssemblyComponentsExternal,
    /// Virtual components stored inside the assembly document whose bodies are
    /// not transferred: the document is absent, its section gives no decoded
    /// body, or its section shares a name with another.
    AssemblyVirtualComponentsNotTransferred,
    /// Suppressed component instances, and the instances below them, are
    /// omitted from the occurrence tree.
    AssemblySuppressedComponentsOmitted,
    /// A component instance has no valid identity, placement, or component
    /// model; the instance is omitted.
    AssemblyPlacementUnresolved,
    /// Assembly configurations other than the active one are not transferred.
    AssemblyConfigurationsNotTransferred,
    /// Component documents embedded in the assembly document are not decoded.
    AssemblyEmbeddedDocumentsNotTransferred,
    /// An occurrence's exclusion from the bill of materials is not transferred.
    AssemblyBomExclusionNotTransferred,
    /// Design features whose operation the decoder typed in its own records
    /// state their family without operands.
    DesignFeatureUnresolved,
    /// User datums whose decoded definition the design types no
    /// construction for state their family without geometry.
    DesignDatumUnresolved,
    /// A feature folder whose members regeneration order does not keep in
    /// one run is not stated.
    DesignFolderDropped,
    /// A stored producer falls outside the shared CADIR timeline position.
    DesignStateNameNative,
    /// Parameters whose literal and value are their dimension's displayed
    /// number, which Keywords rounds to the display precision.
    DesignParameterDisplayRounded,
    /// Parameters whose dimension text is a bare number and that hold no
    /// value: no record types the dimension or binds it to a stored value,
    /// or the part's length unit is not read.
    DesignParameterValueUnread,
    /// Model parameters whose owner is a feature-tree node outside the
    /// timeline (the annotations node, a detail folder), which no design
    /// feature restates, state no design parameter.
    DesignParameterOwnerOutsideTimeline,
    /// Configurations of a part whose active configuration does not resolve
    /// to one of them state no design configuration: a scope with
    /// configurations names its active one.
    DesignConfigurationActiveUnread,
    /// Bodies that the geometry of a part whose feature tree holds no
    /// feature holds outside the active configuration's bodies are unproduced
    /// bodies of its scope: a scope with no root container has no position
    /// for the stored state of a configuration that is not active.
    DesignConfigurationStateUnplaced,
    /// Configurations that are not active state no suppression of their
    /// own: the decoder reads one suppression per feature, from the feature
    /// tree, which every configuration shares (DI-110).
    DesignConfigurationSuppressionUnread,
    /// Features whose definition in a configuration that is not active
    /// differs from the active configuration's state no definition override:
    /// a dimension alone also makes the decoded definitions differ.
    DesignConfigurationDefinitionUnstated,
    /// A configuration parameter sample lacks a stated expression or identity.
    DesignConfigurationParameterUnread,
    /// Features under a feature-tree node outside the timeline (the
    /// annotations node, a lights folder) state no design feature.
    DesignFeatureOutsideTimeline,
    /// Reference-dimension parameters of the model that no sketch dimension
    /// of the design binds state no design parameter: a `Driven` parameter
    /// is bound by its dimension (§5.12).
    DesignParameterReferenceUnbound,
    /// Parameters whose text is one reference to a parameter whose kind
    /// differs or is not read, and whose unit-blind tree does not type or is
    /// not confirmed by the document's values, hold the text and state no
    /// dependency on the parameter it names.
    DesignParameterReferenceAsText,
    /// Extrudes whose blind depth is bound to their record's only dimension,
    /// whose name is neither `Depth` nor `D1`.
    DesignExtrudeDepthAssumed,
    /// Dimensions whose kind is the parameter class a compound-file lane's
    /// own declarations alone give their scalar's class token, which assumes
    /// the token is the lane's own.
    DesignParameterClassAssumed,
    /// Dimensions whose scalars' parameter classes the decoder's class
    /// sources name differently, and whose length the equation manager's
    /// stored value gives where their display shows it.
    DesignParameterClassesDisagree,
    /// Equation-manager lines that the decoder does not read into the
    /// expression of the parameter they assign.
    DesignEquationUnread,
    /// Parameters whose equation-manager line is stated as `SolidWorks`
    /// source text.
    DesignEquationSource,
    /// Sketch curves stated construction because their curve record has
    /// state `0`, read as a curve outside the profile.
    DesignSketchConstructionAssumed,
    /// Typed extrudes whose `Sketch` pick's closed regions differ from the
    /// profile loops read from the stored records.
    DesignExtrudeProfileDiffers,
    /// Typed extrudes whose sketch holds no profile curve, so their `Sketch`
    /// pick takes no region.
    DesignExtrudeSketchEmpty,
    /// Typed extrudes whose `Sketch` pick's closed regions the decoder did
    /// not compare with the stored profile loops.
    DesignExtrudeProfileUnchecked,
    /// Sketch circles built from a dimension record that the design leaves
    /// out, its dimension naming the stored curve they lie on, or states
    /// construction where no face of the solid witnesses them.
    DesignSketchDimensionCircle,
    /// Sketch relations on an end of a closed or unbounded curve, the center
    /// of a curve that has none, or a point whose minted id another entity
    /// holds: they state the curve.
    DesignSketchLocusUnplaced,
    /// Points of sketch curves where two or more point entities lie that the
    /// source names alike: no point entity holds the role there, and a
    /// relation on the point names a point minted there.
    DesignSketchLocusAmbiguous,
    /// Sketch relations that name one point entity for both operands of a
    /// coincidence: the point states the coincidence, and the design holds
    /// no relation record for it.
    DesignSketchRelationSinglePoint,
    /// Sketch relations with no typed relation of their operands and value:
    /// they state their native kind.
    DesignSketchRelationNative,
    /// Sketch entities with no planar curve, stated by their native kind
    /// with no solved curve.
    DesignSketchEntityNative,
    /// Sketch dimensions that bind no parameter of the design: they state
    /// the value their parameter record stores, or no value.
    DesignSketchDimensionUnbound,
    /// Typed revolves whose axis picks the line of their profile sketch that
    /// lies on the stored axis; the decoder does not read which entity the
    /// record's axis reference names.
    DesignRevolveAxisLineInferred,
    /// Typed revolves whose axis no record stores: the decoder derived it
    /// from the sketch's records or the solid's surfaces, and the axis states
    /// the line of the profile sketch that lies on it.
    DesignRevolveAxisDerived,
    /// Typed revolves whose boolean the design leaves unstated: the decoder
    /// reads no operation, or a revolve adds material after an earlier member
    /// and no read field tells a new body from a join.
    DesignRevolveBooleanUnstated,
    /// Typed revolves whose `Sketch` pick's closed regions differ from the
    /// profile loops read from the stored records.
    DesignRevolveProfileDiffers,
    /// Typed revolves whose `Sketch` pick's closed regions the decoder did
    /// not compare with the stored profile loops.
    DesignRevolveProfileUnchecked,
    /// Revolves whose solid depends on the turn sense, a partial turn on one
    /// side or a two-sided turn, whose decoded axis turns about the axis
    /// direction their record stores, reversed where their record's reverse
    /// byte is `1`.
    DesignRevolveSenseAssumed,
    /// Typed reference planes whose frame comes from their mid-plane
    /// constraint: their origin and x axis are a frame the decoder
    /// constructs on the plane.
    DesignDatumPlaneAxisConstructed,
    /// Typed reference planes with an angular `D1` whose x axis is the
    /// u-axis of their angular frame; offset planes on them can store
    /// another u-axis for it (open item DI-82).
    DesignDatumPlaneAxisAngled,
    /// Typed datums whose geometry decodes differently in a configuration:
    /// the datum states the active configuration's geometry.
    DesignDatumConfigurationDropped,
}

impl SldprtLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [SldprtLossCode] = &[
        Self::MetadataPartRootUnsupported,
        Self::MetadataConfigurationManagerOpaque,
        Self::MetadataDefaultCarrierUnsupported,
        Self::MetadataDefaultOwnerAmbiguous,
        Self::MetadataDefaultAliasAmbiguous,
        Self::MetadataDefaultValueUnsupported,
        Self::MetadataMassPropertiesNotCompared,
        Self::MetadataConfigurationPropertyOwnerAmbiguous,
        Self::MetadataConfigurationPropertyCarrierUnsupported,
        Self::MetadataConfigurationPropertyValueUnsupported,
        Self::ConfigActiveIdentityUnresolved,
        Self::ConfigActivePartitionMismatch,
        Self::ConfigActivePartitionUnread,
        Self::ConfigInferredWithoutNative,
        Self::ConfigLaneIdentityUnresolved,
        Self::ConfigAmbiguousPartition,
        Self::ConfigAmbiguousNaming,
        Self::ConfigIncoherentBodyRefs,
        Self::ConfigIncompleteSnapshot,
        Self::ConfigProjectionWorkCapped,
        Self::ConfigStateUnstated,
        Self::ConfigStateNotStored,
        Self::ConfigStatePresentationUnstated,
        Self::ConfigStateImportedBodiesNotTransferred,
        Self::ConfigStateOutputsUnstated,
        Self::ParameterUnevaluated,
        Self::ParameterAmbiguousIdentity,
        Self::PmiDimensionUnbound,
        Self::PmiSemanticParseCapped,
        Self::PmiSwiftAnnotationUnsupported,
        Self::PmiSwiftFeatureWalkCapped,
        Self::PmiSwiftImplicitNominalUnresolved,
        Self::PmiSwiftPositionTargetUnresolved,
        Self::PmiSwiftSphericalDiameterTargetCountTextEqualRadiusAssumed,
        Self::PmiSwiftSphericalDiameterTargetUnresolved,
        Self::PmiSwiftPresentationUnresolved,
        Self::PmiSwiftInternalFlatnessSymbolAssumed,
        Self::PmiSwiftPresentationUnframed,
        Self::PmiSwiftFaceDocumentLayoutAssumed,
        Self::PmiSwiftFaceDocumentTailAssumed,
        Self::HistoryIncompleteReferences,
        Self::HistoryRollbackUnread,
        Self::HistoryStatesUnread,
        Self::HistoryRollbackMarkClassInferred,
        Self::HistoryApplicationNotTransferred,
        Self::HistoryApplicationNestingCut,
        Self::HistoryXmlNormalised,
        Self::FeatureIncoherentEdges,
        Self::FeatureIncoherentContent,
        Self::FeatureUnresolvedOutputScope,
        Self::FeatureIncoherentOutputs,
        Self::SketchNativeConstraint,
        Self::SketchNativeGeometry,
        Self::SketchHandleUnresolved,
        Self::SketchLineDegenerate,
        Self::SketchArcRecordConflict,
        Self::SketchMarkerRecordUnread,
        Self::SketchSecondReading,
        Self::SketchMarkerReadingUnbound,
        Self::SketchPointCoordinatesSubnormal,
        Self::SketchRelationPointPlaceUnsettled,
        Self::SketchPlaneBindingAssumed,
        Self::SketchPointRecordIndexBasisAssumed,
        Self::SketchArcRecordOffChord,
        Self::SketchProfileRejected,
        Self::SketchListedLineConstructionAssumed,
        Self::SketchListedCurveRelationDropped,
        Self::SketchRelationUnprojected,
        Self::SketchRelationMultiplyProjected,
        Self::SketchDimensionClassUnknown,
        Self::SketchDimensionRepeated,
        Self::FeatureNativeKindRetained,
        Self::FeatureInputObjectUnbound,
        Self::FeatureInputSolidBodyRetained,
        Self::FeatureInputArchiveWalkStopped,
        Self::FeatureBlendEdgeGeneratorUnstated,
        Self::FeatureBlendMarkReferencesAssumed,
        Self::FeatureTypedOperandIncomplete,
        Self::FeatureSweepPathRetained,
        Self::FeatureSweepPathRosterAssumed,
        Self::FeatureSweepPathHeaderAssumed,
        Self::FeatureSweepMergeAssumed,
        Self::FeatureProfileCurvesUnavailable,
        Self::FeatureExtrudeDirectionUnresolved,
        Self::FeatureExtrudeReversalContradicted,
        Self::FeatureExtrudeReversalAssumed,
        Self::FeatureExtrudeStartUnresolved,
        Self::FeatureOffsetPlaneSideAssumed,
        Self::FeatureOffsetPlaneFrameConflict,
        Self::FeatureOffsetPlaneDistanceDisplayed,
        Self::FeatureRevolveAxisOffProfilePlane,
        Self::FeatureRevolveAxisRecordPrefixUnread,
        Self::FeatureThinOperationAmbiguous,
        Self::FeatureEdgeTerminalUnresolved,
        Self::FeatureBodyPathAmbiguous,
        Self::FeatureBodyRetentionUnresolved,
        Self::GeometryFaceSupportSurfaceUntyped,
        Self::GeometrySurfaceCarrierAmbiguous,
        Self::GeometryOffsetValidityUnresolved,
        Self::GeometrySpunProfileIntervalIgnored,
        Self::GeometryBlendCacheUnsolved,
        Self::GeometryEdgeSupportCurveUntyped,
        Self::GeometryTolerantEdgeCurveUnread,
        Self::GeometryPointRecordAmbiguous,
        Self::GeometryCurveMarkerConflict,
        Self::GeometryPcurveAmbiguous,
        Self::GeometrySurfaceSolverWorkCapped,
        Self::GeometrySplineLanesUnpaired,
        Self::GeometrySplineWeightNonpositive,
        Self::GeometrySplineFieldsInconsistent,
        Self::GeometryConeHalfAngleSignUnresolved,
        Self::GeometryConeHalfAngleIdentityRefused,
        Self::GeometryIntersectionLayoutUnknown,
        Self::GeometryCarrierApproximated,
        Self::AppearanceFaceColorUnresolved,
        Self::AppearanceAssignmentUnresolved,
        Self::NativeAppearanceScopeUnresolved,
        Self::TessellationFaceOwnershipUnresolved,
        Self::TessellationOwnerWorkCapped,
        Self::TessellationReusedTagAmbiguous,
        Self::TessellationDisplayWithoutGeometry,
        Self::TopologyBodyHierarchyDerived,
        Self::TopologyLoopChainCut,
        Self::TopologyOwnershipWithheld,
        Self::TopologyShellRoleUnstated,
        Self::TopologyFinRecordAmbiguous,
        Self::TopologyFinPartnerUnresolved,
        Self::TopologyLoopRecordAmbiguous,
        Self::TopologyVertexRecordUnresolved,
        Self::TopologyPcurveCarrierOffSurface,
        Self::GeometryParasolidNotTransferred,
        Self::GeometryMeshBodyNotTransferred,
        Self::GeometryImportedBodiesNotTransferred,
        Self::BodyResolutionInvalid,
        Self::BodyResolutionUnread,
        Self::TopologyGraphNotTransferred,
        Self::MaterialMetadataNotTransferred,
        Self::ContainerNoParasolidStream,
        Self::ContainerParasolidProbeCapped,
        Self::ContainerNoncanonical,
        Self::SourcePreservedImageUnavailable,
        Self::SourceDialectUnverified,
        Self::KernelDialectUnverified,
        Self::DialectLayerCollision,
        Self::WriterShellWireTopologyOmitted,
        Self::WriterCoedgePcurveUsesOmitted,
        Self::WriterSphereSeamCurvesOmitted,
        Self::WriterAppearanceBindingsOmitted,
        Self::WriterAttributesOmitted,
        Self::WriterDrivenDimensionsOmitted,
        Self::WriterUnboundAppearancesOmitted,
        Self::WriterMaterialsOmitted,
        Self::WriterTessellationMetadataOmitted,
        Self::WriterNativeEnvelopeAuxiliaryOmitted,
        Self::WriterUnitsMemberGenerated,
        Self::WriterNativeHistorySummary,
        Self::WriterNativeSkeletonUnlaidRuns,
        Self::WriterNativeSkeletonOmitted,
        Self::WriterNativeStreamsOmitted,
        Self::WriterNativeSketchLaneOrphan,
        Self::WriterNativeBodyIdentityOmitted,
        Self::WriterProducerAttributesOmitted,
        Self::WriterNativeSketchProfileBodyKept,
        Self::WriterNativeSketchOriginRelationAdded,
        Self::WriterConfigurationIdentityAssigned,
        Self::WriterBodiesOmitted,
        Self::WriterFaceNamesOmitted,
        Self::WriterBodyNamesOmitted,
        Self::WriterAppearanceFieldsOmitted,
        Self::WriterSubdOmitted,
        Self::WriterActiveConfigurationAssumed,
        Self::WriterFeaturesOmitted,
        Self::WriterFeatureResultAbsent,
        Self::WriterSketchConstraintsOmitted,
        Self::WriterEdgeRangesOmitted,
        Self::WriterPmiOmitted,
        Self::WriterParameterExpressionsOmitted,
        Self::WriterCacheTolerancesOmitted,
        Self::WriterFaceLoopsUnchained,
        Self::WriterSelectionIdentitiesOmitted,
        Self::WriterFeatureFieldsOmitted,
        Self::WriterParametersRenamed,
        Self::WriterKeywordsCharactersReplaced,
        Self::WriterKeywordsTextTrimmed,
        Self::WriterHistoryKeywordsOnly,
        Self::WriterHistoryNameNotWritten,
        Self::WriterHistoryRegenerationRefused,
        Self::WriterSolidStaleAfterEdit,
        Self::WriterConfigurationSnapshotEditUnwritten,
        Self::WriterFeatureRecordReshaped,
        Self::WriterReadBackDiffers,
        Self::WriterReadBackUnread,
        Self::WriterExtrudeReversalUnwritten,
        Self::WriterExtrudeReversalConfigurationCleared,
        Self::SourcePropertyKeyBlank,
        Self::AssemblyStructureNotTransferred,
        Self::AssemblyMatesNotTransferred,
        Self::AssemblyComponentsExternal,
        Self::AssemblyVirtualComponentsNotTransferred,
        Self::AssemblySuppressedComponentsOmitted,
        Self::AssemblyPlacementUnresolved,
        Self::AssemblyConfigurationsNotTransferred,
        Self::AssemblyEmbeddedDocumentsNotTransferred,
        Self::AssemblyBomExclusionNotTransferred,
        Self::DesignFeatureUnresolved,
        Self::DesignDatumUnresolved,
        Self::DesignFolderDropped,
        Self::DesignStateNameNative,
        Self::DesignParameterDisplayRounded,
        Self::DesignParameterValueUnread,
        Self::DesignParameterOwnerOutsideTimeline,
        Self::DesignConfigurationActiveUnread,
        Self::DesignConfigurationStateUnplaced,
        Self::DesignConfigurationSuppressionUnread,
        Self::DesignConfigurationDefinitionUnstated,
        Self::DesignConfigurationParameterUnread,
        Self::DesignFeatureOutsideTimeline,
        Self::DesignParameterReferenceUnbound,
        Self::DesignParameterReferenceAsText,
        Self::DesignExtrudeDepthAssumed,
        Self::DesignParameterClassAssumed,
        Self::DesignParameterClassesDisagree,
        Self::DesignEquationUnread,
        Self::DesignEquationSource,
        Self::DesignSketchConstructionAssumed,
        Self::DesignExtrudeProfileDiffers,
        Self::DesignExtrudeSketchEmpty,
        Self::DesignExtrudeProfileUnchecked,
        Self::DesignSketchDimensionCircle,
        Self::DesignSketchLocusUnplaced,
        Self::DesignSketchLocusAmbiguous,
        Self::DesignSketchRelationSinglePoint,
        Self::DesignSketchRelationNative,
        Self::DesignSketchEntityNative,
        Self::DesignSketchDimensionUnbound,
        Self::DesignRevolveAxisLineInferred,
        Self::DesignRevolveAxisDerived,
        Self::DesignRevolveBooleanUnstated,
        Self::DesignRevolveProfileDiffers,
        Self::DesignRevolveProfileUnchecked,
        Self::DesignRevolveSenseAssumed,
        Self::DesignDatumPlaneAxisConstructed,
        Self::DesignDatumPlaneAxisAngled,
        Self::DesignDatumConfigurationDropped,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::ConfigActiveIdentityUnresolved => "config.active-identity-unresolved",
            Self::ConfigActivePartitionMismatch => "config.active-partition-mismatch",
            Self::ConfigActivePartitionUnread => "config.active-partition-unread",
            Self::ConfigInferredWithoutNative => "config.inferred-without-native",
            Self::ConfigLaneIdentityUnresolved => "config.lane-identity-unresolved",
            Self::ConfigAmbiguousPartition => "config.ambiguous-partition",
            Self::ConfigAmbiguousNaming => "config.ambiguous-naming",
            Self::ConfigIncoherentBodyRefs => "config.incoherent-body-refs",
            Self::ConfigIncompleteSnapshot => "config.incomplete-snapshot",
            Self::ConfigProjectionWorkCapped => "config.projection-work-capped",
            Self::ConfigStateUnstated => "config.state-unstated",
            Self::ConfigStateNotStored => "config.state-not-stored",
            Self::ConfigStatePresentationUnstated => "config.state-presentation-unstated",
            Self::ConfigStateImportedBodiesNotTransferred => {
                "config.state-imported-bodies-not-transferred"
            }
            Self::ConfigStateOutputsUnstated => "config.state-outputs-unstated",
            Self::ParameterUnevaluated => "parameter.unevaluated",
            Self::ParameterAmbiguousIdentity => "parameter.ambiguous-identity",
            Self::PmiDimensionUnbound => "pmi.dimension-unbound",
            Self::PmiSemanticParseCapped => "pmi.semantic-parse-capped",
            Self::PmiSwiftAnnotationUnsupported => "pmi.swift-annotation-unsupported",
            Self::PmiSwiftFeatureWalkCapped => "pmi.swift-feature-walk-capped",
            Self::PmiSwiftImplicitNominalUnresolved => "pmi.swift-implicit-nominal-unresolved",
            Self::PmiSwiftPositionTargetUnresolved => "pmi.swift-position-target-unresolved",
            Self::PmiSwiftSphericalDiameterTargetCountTextEqualRadiusAssumed => {
                "pmi.swift-spherical-diameter-count-text-equal-radius-assumed"
            }
            Self::PmiSwiftSphericalDiameterTargetUnresolved => {
                "pmi.swift-spherical-diameter-target-unresolved"
            }
            Self::PmiSwiftPresentationUnresolved => "pmi.swift-presentation-unresolved",
            Self::PmiSwiftInternalFlatnessSymbolAssumed => {
                "pmi.swift-internal-flatness-symbol-assumed"
            }
            Self::PmiSwiftPresentationUnframed => "pmi.swift-presentation-unframed",
            Self::PmiSwiftFaceDocumentLayoutAssumed => "pmi.swift-face-document-layout-assumed",
            Self::PmiSwiftFaceDocumentTailAssumed => "pmi.swift-face-document-tail-assumed",
            Self::HistoryIncompleteReferences => "history.incomplete-references",
            Self::HistoryRollbackUnread => "history.rollback-unread",
            Self::HistoryStatesUnread => "history.states-unread",
            Self::HistoryRollbackMarkClassInferred => "history.rollback-mark-class-inferred",
            Self::HistoryApplicationNotTransferred => "history.application-not-transferred",
            Self::HistoryApplicationNestingCut => "history.application-nesting-cut",
            Self::HistoryXmlNormalised => "history.xml-normalised",
            Self::FeatureIncoherentEdges => "feature.incoherent-edges",
            Self::FeatureIncoherentContent => "feature.incoherent-content",
            Self::FeatureUnresolvedOutputScope => "feature.unresolved-output-scope",
            Self::FeatureIncoherentOutputs => "feature.incoherent-outputs",
            Self::SketchNativeConstraint => "sketch.native-constraint",
            Self::SketchNativeGeometry => "sketch.native-geometry",
            Self::SketchProfileRejected => "sketch.profile-rejected",
            Self::SketchListedLineConstructionAssumed => "sketch.listed-line-construction-assumed",
            Self::SketchListedCurveRelationDropped => "sketch.listed-curve-relation-dropped",
            Self::SketchHandleUnresolved => "sketch.handle-unresolved",
            Self::SketchLineDegenerate => "sketch.line-degenerate",
            Self::SketchArcRecordConflict => "sketch.arc-record-conflict",
            Self::SketchMarkerRecordUnread => "sketch.marker-record-unread",
            Self::SketchSecondReading => "sketch.second-reading",
            Self::SketchMarkerReadingUnbound => "sketch.marker-reading-unbound",
            Self::SketchPointCoordinatesSubnormal => "sketch.point-coordinates-subnormal",
            Self::SketchRelationPointPlaceUnsettled => "sketch.relation-point-place-unsettled",
            Self::SketchPlaneBindingAssumed => "sketch.plane-binding-assumed",
            Self::SketchPointRecordIndexBasisAssumed => "sketch.point-record-index-basis-assumed",
            Self::SketchArcRecordOffChord => "sketch.arc-record-off-chord",
            Self::SketchRelationUnprojected => "sketch.relation-unprojected",
            Self::SketchRelationMultiplyProjected => "sketch.relation-multiply-projected",
            Self::SketchDimensionClassUnknown => "sketch.dimension-class-unknown",
            Self::SketchDimensionRepeated => "sketch.dimension-repeated",
            Self::FeatureNativeKindRetained => "feature.native-kind-retained",
            Self::FeatureInputObjectUnbound => "feature.input-object-unbound",
            Self::FeatureInputSolidBodyRetained => "feature.input-solid-body-retained",
            Self::FeatureInputArchiveWalkStopped => "feature.input-archive-walk-stopped",
            Self::FeatureBlendEdgeGeneratorUnstated => "feature.blend-edge-generator-unstated",
            Self::FeatureBlendMarkReferencesAssumed => "feature.blend-mark-references-assumed",
            Self::FeatureTypedOperandIncomplete => "feature.typed-operand-incomplete",
            Self::FeatureSweepPathRetained => "feature.sweep-path-retained",
            Self::FeatureSweepPathRosterAssumed => "feature.sweep-path-roster-assumed",
            Self::FeatureSweepPathHeaderAssumed => "feature.sweep-path-header-assumed",
            Self::FeatureSweepMergeAssumed => "feature.sweep-merge-assumed",
            Self::FeatureProfileCurvesUnavailable => "feature.profile-curves-unavailable",
            Self::FeatureExtrudeDirectionUnresolved => "feature.extrude-direction-unresolved",
            Self::FeatureExtrudeReversalContradicted => "feature.extrude-reversal-contradicted",
            Self::FeatureExtrudeReversalAssumed => "feature.extrude-reversal-assumed",
            Self::FeatureExtrudeStartUnresolved => "feature.extrude-start-unresolved",
            Self::FeatureOffsetPlaneSideAssumed => "feature.offset-plane-side-assumed",
            Self::FeatureOffsetPlaneFrameConflict => "feature.offset-plane-frame-conflict",
            Self::FeatureOffsetPlaneDistanceDisplayed => "feature.offset-plane-distance-displayed",
            Self::FeatureRevolveAxisOffProfilePlane => "feature.revolve-axis-off-profile-plane",
            Self::FeatureRevolveAxisRecordPrefixUnread => {
                "feature.revolve-axis-record-prefix-unread"
            }
            Self::FeatureThinOperationAmbiguous => "feature.thin-operation-ambiguous",
            Self::FeatureEdgeTerminalUnresolved => "feature.edge-terminal-unresolved",
            Self::FeatureBodyPathAmbiguous => "feature.body-path-ambiguous",
            Self::FeatureBodyRetentionUnresolved => "feature.body-retention-unresolved",
            Self::GeometryFaceSupportSurfaceUntyped => "geometry.face-support-surface-untyped",
            Self::GeometrySurfaceCarrierAmbiguous => "geometry.surface-carrier-ambiguous",
            Self::GeometryOffsetValidityUnresolved => "geometry.offset-validity-unresolved",
            Self::GeometrySpunProfileIntervalIgnored => "geometry.spun-profile-interval-ignored",
            Self::GeometryBlendCacheUnsolved => "geometry.blend-cache-unsolved",
            Self::GeometryEdgeSupportCurveUntyped => "geometry.edge-support-curve-untyped",
            Self::GeometryTolerantEdgeCurveUnread => "geometry.tolerant-edge-curve-unread",
            Self::GeometryPointRecordAmbiguous => "geometry.point-record-ambiguous",
            Self::GeometryCurveMarkerConflict => "geometry.curve-marker-conflict",
            Self::GeometryPcurveAmbiguous => "geometry.pcurve-ambiguous",
            Self::GeometrySurfaceSolverWorkCapped => "geometry.surface-solver-work-capped",
            Self::GeometrySplineLanesUnpaired => "geometry.spline-lanes-unpaired",
            Self::GeometrySplineWeightNonpositive => "geometry.spline-weight-nonpositive",
            Self::GeometrySplineFieldsInconsistent => "geometry.spline-fields-inconsistent",
            Self::GeometryConeHalfAngleSignUnresolved => "geometry.cone-half-angle-sign-unresolved",
            Self::GeometryConeHalfAngleIdentityRefused => {
                "geometry.cone-half-angle-identity-refused"
            }
            Self::GeometryIntersectionLayoutUnknown => "geometry.intersection-layout-unknown",
            Self::GeometryCarrierApproximated => "geometry.carrier-approximated",
            Self::AppearanceFaceColorUnresolved => "appearance.face-color-unresolved",
            Self::AppearanceAssignmentUnresolved => "appearance.assignment-unresolved",
            Self::NativeAppearanceScopeUnresolved => "appearance.native-scope-unresolved",
            Self::TessellationFaceOwnershipUnresolved => "tessellation.face-ownership-unresolved",
            Self::TessellationOwnerWorkCapped => "tessellation.owner-work-capped",
            Self::TessellationReusedTagAmbiguous => "tessellation.reused-tag-ambiguous",
            Self::TessellationDisplayWithoutGeometry => "tessellation.display-without-geometry",
            Self::TopologyBodyHierarchyDerived => "topology.body-hierarchy-derived",
            Self::TopologyOwnershipWithheld => "topology.ownership-withheld",
            Self::TopologyShellRoleUnstated => "topology.shell-role-unstated",
            Self::TopologyLoopChainCut => "topology.loop-chain-cut",
            Self::TopologyFinRecordAmbiguous => "topology.fin-record-ambiguous",
            Self::TopologyFinPartnerUnresolved => "topology.fin-partner-unresolved",
            Self::TopologyLoopRecordAmbiguous => "topology.loop-record-ambiguous",
            Self::TopologyVertexRecordUnresolved => "topology.vertex-record-unresolved",
            Self::TopologyPcurveCarrierOffSurface => "topology.pcurve-carrier-off-surface",
            Self::GeometryParasolidNotTransferred => "geometry.parasolid-not-transferred",
            Self::GeometryMeshBodyNotTransferred => "geometry.mesh-body-not-transferred",
            Self::GeometryImportedBodiesNotTransferred => {
                "geometry.imported-bodies-not-transferred"
            }
            Self::BodyResolutionInvalid => "body.resolution-invalid",
            Self::BodyResolutionUnread => "body.resolution-unread",
            Self::TopologyGraphNotTransferred => "topology.graph-not-transferred",
            Self::MaterialMetadataNotTransferred => "material.metadata-not-transferred",
            Self::ContainerNoParasolidStream => "container.no-parasolid-stream",
            Self::ContainerParasolidProbeCapped => "container.parasolid-probe-capped",
            Self::ContainerNoncanonical => "container.noncanonical",
            Self::SourcePreservedImageUnavailable => "source.preserved-image-unavailable",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::KernelDialectUnverified => "source.kernel-dialect-unverified",
            Self::DialectLayerCollision => "source.dialect-layer-collision",
            Self::WriterShellWireTopologyOmitted => "writer.shell-wire-topology-omitted",
            Self::WriterCoedgePcurveUsesOmitted => "writer.coedge-pcurve-uses-omitted",
            Self::WriterSphereSeamCurvesOmitted => "writer.sphere-seam-curves-omitted",
            Self::WriterAppearanceBindingsOmitted => "writer.appearance-bindings-omitted",
            Self::WriterAttributesOmitted => "writer.attributes-omitted",
            Self::WriterDrivenDimensionsOmitted => "writer.driven-dimensions-omitted",
            Self::WriterUnboundAppearancesOmitted => "writer.unbound-appearances-omitted",
            Self::WriterMaterialsOmitted => "writer.materials-omitted",
            Self::WriterTessellationMetadataOmitted => "writer.tessellation-metadata-omitted",
            Self::WriterNativeEnvelopeAuxiliaryOmitted => {
                "writer.native-envelope-auxiliary-omitted"
            }
            Self::WriterUnitsMemberGenerated => "writer.units-member-generated",
            Self::WriterNativeHistorySummary => "writer.native-history-summary",
            Self::WriterNativeSkeletonUnlaidRuns => "writer.native-skeleton-unlaid-runs",
            Self::WriterNativeSkeletonOmitted => "writer.native-skeleton-omitted",
            Self::WriterNativeStreamsOmitted => "writer.native-streams-omitted",
            Self::WriterNativeSketchLaneOrphan => "writer.native-sketch-lane-orphan",
            Self::WriterNativeBodyIdentityOmitted => "writer.native-body-identity-omitted",
            Self::WriterProducerAttributesOmitted => "writer.producer-attributes-omitted",
            Self::WriterNativeSketchProfileBodyKept => "writer.native-sketch-profile-body-kept",
            Self::WriterNativeSketchOriginRelationAdded => {
                "writer.native-sketch-origin-relation-added"
            }
            Self::WriterConfigurationIdentityAssigned => "writer.configuration-identity-assigned",
            Self::WriterBodiesOmitted => "writer.bodies-omitted",
            Self::WriterFaceNamesOmitted => "writer.face-names-omitted",
            Self::WriterBodyNamesOmitted => "writer.body-names-omitted",
            Self::WriterAppearanceFieldsOmitted => "writer.appearance-fields-omitted",
            Self::WriterSubdOmitted => "writer.subd-omitted",
            Self::WriterActiveConfigurationAssumed => "writer.active-configuration-assumed",
            Self::WriterFeaturesOmitted => "writer.features-omitted",
            Self::WriterFeatureResultAbsent => "writer.feature-result-absent",
            Self::WriterSketchConstraintsOmitted => "writer.sketch-constraints-omitted",
            Self::WriterEdgeRangesOmitted => "writer.edge-ranges-omitted",
            Self::WriterPmiOmitted => "writer.pmi-omitted",
            Self::WriterParameterExpressionsOmitted => "writer.parameter-expressions-omitted",
            Self::WriterCacheTolerancesOmitted => "writer.cache-tolerances-omitted",
            Self::WriterFaceLoopsUnchained => "writer.face-loops-unchained",
            Self::WriterSelectionIdentitiesOmitted => "writer.selection-identities-omitted",
            Self::WriterFeatureFieldsOmitted => "writer.feature-fields-omitted",
            Self::WriterParametersRenamed => "writer.parameters-renamed",
            Self::WriterKeywordsCharactersReplaced => "writer.keywords-characters-replaced",
            Self::WriterKeywordsTextTrimmed => "writer.keywords-text-trimmed",
            Self::WriterHistoryKeywordsOnly => "writer.history-keywords-only",
            Self::WriterHistoryNameNotWritten => "writer.history-name-not-written",
            Self::WriterHistoryRegenerationRefused => "writer.history-regeneration-refused",
            Self::WriterSolidStaleAfterEdit => "writer.solid-stale-after-edit",
            Self::WriterConfigurationSnapshotEditUnwritten => {
                "writer.configuration-snapshot-edit-unwritten"
            }
            Self::WriterFeatureRecordReshaped => "writer.feature-record-reshaped",
            Self::WriterReadBackDiffers => "writer.read-back-differs",
            Self::WriterReadBackUnread => "writer.read-back-unread",
            Self::WriterExtrudeReversalUnwritten => "writer.extrude-reversal-unwritten",
            Self::WriterExtrudeReversalConfigurationCleared => {
                "writer.extrude-reversal-configuration-cleared"
            }
            Self::SourcePropertyKeyBlank => "source.property-key-blank",
            Self::MetadataPartRootUnsupported => "metadata.part-root-unsupported",
            Self::MetadataConfigurationManagerOpaque => "metadata.configuration-manager-opaque",
            Self::MetadataDefaultCarrierUnsupported => "metadata.default-carrier-unsupported",
            Self::MetadataDefaultOwnerAmbiguous => "metadata.default-owner-ambiguous",
            Self::MetadataDefaultAliasAmbiguous => "metadata.default-alias-ambiguous",
            Self::MetadataDefaultValueUnsupported => "metadata.default-value-unsupported",
            Self::MetadataMassPropertiesNotCompared => "metadata.mass-properties-not-compared",
            Self::MetadataConfigurationPropertyOwnerAmbiguous => {
                "metadata.configuration-property-owner-ambiguous"
            }
            Self::MetadataConfigurationPropertyCarrierUnsupported => {
                "metadata.configuration-property-carrier-unsupported"
            }
            Self::MetadataConfigurationPropertyValueUnsupported => {
                "metadata.configuration-property-value-unsupported"
            }
            Self::AssemblyStructureNotTransferred => "assembly.structure-not-transferred",
            Self::AssemblyMatesNotTransferred => "assembly.mates-not-transferred",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::AssemblyVirtualComponentsNotTransferred => {
                "assembly.virtual-components-not-transferred"
            }
            Self::AssemblySuppressedComponentsOmitted => "assembly.suppressed-components-omitted",
            Self::AssemblyPlacementUnresolved => "assembly.placement-unresolved",
            Self::AssemblyConfigurationsNotTransferred => "assembly.configurations-not-transferred",
            Self::AssemblyEmbeddedDocumentsNotTransferred => {
                "assembly.embedded-documents-not-transferred"
            }
            Self::AssemblyBomExclusionNotTransferred => "assembly.bom-exclusion-not-transferred",
            Self::DesignFeatureUnresolved => "design.feature-unresolved",
            Self::DesignDatumUnresolved => "design.datum-unresolved",
            Self::DesignFolderDropped => "design.folder-dropped",
            Self::DesignStateNameNative => "design.state-name-native",
            Self::DesignParameterDisplayRounded => "design.parameter-display-rounded",
            Self::DesignParameterValueUnread => "design.parameter-value-unread",
            Self::DesignParameterOwnerOutsideTimeline => "design.parameter-owner-outside-timeline",
            Self::DesignConfigurationActiveUnread => "design.configuration-active-unread",
            Self::DesignConfigurationStateUnplaced => "design.configuration-state-unplaced",
            Self::DesignConfigurationSuppressionUnread => "design.configuration-suppression-unread",
            Self::DesignConfigurationParameterUnread => "design.configuration-parameter-unread",
            Self::DesignConfigurationDefinitionUnstated => {
                "design.configuration-definition-unstated"
            }
            Self::DesignFeatureOutsideTimeline => "design.feature-outside-timeline",
            Self::DesignParameterReferenceUnbound => "design.parameter-reference-unbound",
            Self::DesignParameterReferenceAsText => "design.parameter-reference-as-text",
            Self::DesignExtrudeDepthAssumed => "design.extrude-depth-assumed",
            Self::DesignParameterClassAssumed => "design.parameter-class-assumed",
            Self::DesignParameterClassesDisagree => "design.parameter-classes-disagree",
            Self::DesignEquationUnread => "design.equation-unread",
            Self::DesignEquationSource => "design.equation-source",
            Self::DesignSketchConstructionAssumed => "design.sketch-construction-assumed",
            Self::DesignExtrudeProfileDiffers => "design.extrude-profile-differs",
            Self::DesignExtrudeSketchEmpty => "design.extrude-sketch-empty",
            Self::DesignExtrudeProfileUnchecked => "design.extrude-profile-unchecked",
            Self::DesignRevolveAxisLineInferred => "design.revolve-axis-line-inferred",
            Self::DesignRevolveAxisDerived => "design.revolve-axis-derived",
            Self::DesignRevolveBooleanUnstated => "design.revolve-boolean-unstated",
            Self::DesignRevolveProfileDiffers => "design.revolve-profile-differs",
            Self::DesignRevolveProfileUnchecked => "design.revolve-profile-unchecked",
            Self::DesignRevolveSenseAssumed => "design.revolve-sense-assumed",
            Self::DesignDatumPlaneAxisConstructed => "design.datum-plane-axis-constructed",
            Self::DesignDatumPlaneAxisAngled => "design.datum-plane-axis-angled",
            Self::DesignDatumConfigurationDropped => "design.datum-configuration-dropped",
            Self::DesignSketchDimensionCircle => "design.sketch-dimension-circle",
            Self::DesignSketchLocusUnplaced => "design.sketch-locus-unplaced",
            Self::DesignSketchLocusAmbiguous => "design.sketch-locus-ambiguous",
            Self::DesignSketchRelationSinglePoint => "design.sketch-relation-single-point",
            Self::DesignSketchRelationNative => "design.sketch-relation-native",
            Self::DesignSketchEntityNative => "design.sketch-entity-native",
            Self::DesignSketchDimensionUnbound => "design.sketch-dimension-unbound",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::WriterTessellationMetadataOmitted => Severity::Warning,
            Self::BodyResolutionInvalid | Self::BodyResolutionUnread => Severity::Warning,
            Self::WriterNativeEnvelopeAuxiliaryOmitted => Severity::Warning,
            Self::WriterUnitsMemberGenerated => Severity::Warning,
            Self::WriterNativeHistorySummary => Severity::Warning,
            Self::WriterNativeSkeletonUnlaidRuns => Severity::Warning,
            Self::WriterNativeSkeletonOmitted => Severity::Warning,
            Self::WriterNativeStreamsOmitted => Severity::Warning,
            Self::WriterNativeSketchLaneOrphan => Severity::Warning,
            Self::WriterNativeBodyIdentityOmitted => Severity::Warning,
            Self::WriterProducerAttributesOmitted => Severity::Warning,
            Self::WriterNativeSketchProfileBodyKept => Severity::Warning,
            Self::WriterNativeSketchOriginRelationAdded => Severity::Warning,
            Self::WriterConfigurationIdentityAssigned => Severity::Warning,
            Self::WriterBodiesOmitted => Severity::Warning,
            Self::WriterFaceNamesOmitted => Severity::Warning,
            Self::WriterBodyNamesOmitted => Severity::Warning,
            Self::WriterAppearanceFieldsOmitted => Severity::Warning,
            Self::WriterSubdOmitted => Severity::Warning,
            Self::WriterActiveConfigurationAssumed => Severity::Warning,
            Self::WriterFeaturesOmitted => Severity::Warning,
            Self::WriterFeatureResultAbsent => Severity::Warning,
            Self::WriterSketchConstraintsOmitted => Severity::Warning,
            Self::WriterEdgeRangesOmitted => Severity::Warning,
            Self::WriterPmiOmitted => Severity::Warning,
            Self::WriterParameterExpressionsOmitted => Severity::Warning,
            Self::WriterCacheTolerancesOmitted => Severity::Warning,
            Self::WriterFaceLoopsUnchained => Severity::Warning,
            Self::WriterSelectionIdentitiesOmitted => Severity::Warning,
            Self::WriterFeatureFieldsOmitted => Severity::Warning,
            Self::WriterParametersRenamed => Severity::Warning,
            Self::WriterKeywordsCharactersReplaced => Severity::Warning,
            Self::WriterKeywordsTextTrimmed => Severity::Warning,
            Self::WriterHistoryKeywordsOnly => Severity::Warning,
            Self::WriterHistoryNameNotWritten => Severity::Warning,
            Self::WriterHistoryRegenerationRefused => Severity::Error,
            Self::WriterSolidStaleAfterEdit => Severity::Warning,
            Self::WriterConfigurationSnapshotEditUnwritten => Severity::Warning,
            Self::WriterFeatureRecordReshaped => Severity::Warning,
            Self::WriterReadBackDiffers => Severity::Warning,
            Self::WriterReadBackUnread => Severity::Error,
            Self::WriterExtrudeReversalUnwritten => Severity::Warning,
            Self::WriterExtrudeReversalConfigurationCleared => Severity::Warning,
            Self::WriterUnboundAppearancesOmitted => Severity::Warning,
            Self::WriterMaterialsOmitted => Severity::Warning,
            Self::WriterAttributesOmitted => Severity::Warning,
            Self::WriterDrivenDimensionsOmitted => Severity::Warning,
            Self::WriterAppearanceBindingsOmitted => Severity::Warning,
            Self::WriterSphereSeamCurvesOmitted => Severity::Warning,
            Self::WriterCoedgePcurveUsesOmitted => Severity::Warning,
            Self::WriterShellWireTopologyOmitted => Severity::Warning,
            Self::GeometryParasolidNotTransferred
            | Self::TopologyGraphNotTransferred
            | Self::SourcePreservedImageUnavailable => Severity::Blocking,
            Self::ContainerNoParasolidStream
            | Self::GeometryMeshBodyNotTransferred
            | Self::GeometryImportedBodiesNotTransferred => Severity::Error,
            Self::HistoryApplicationNotTransferred | Self::HistoryApplicationNestingCut => {
                Severity::Warning
            }
            Self::ConfigActiveIdentityUnresolved
            | Self::ConfigActivePartitionMismatch
            | Self::ConfigActivePartitionUnread
            | Self::ConfigInferredWithoutNative
            | Self::ConfigLaneIdentityUnresolved
            | Self::ConfigAmbiguousPartition
            | Self::ConfigAmbiguousNaming
            | Self::ConfigIncoherentBodyRefs
            | Self::ConfigIncompleteSnapshot
            | Self::ConfigProjectionWorkCapped
            | Self::ConfigStateUnstated
            | Self::ConfigStateNotStored
            | Self::ConfigStatePresentationUnstated
            | Self::ConfigStateImportedBodiesNotTransferred
            | Self::ConfigStateOutputsUnstated
            | Self::ParameterUnevaluated
            | Self::ParameterAmbiguousIdentity
            | Self::PmiDimensionUnbound
            | Self::PmiSemanticParseCapped
            | Self::PmiSwiftAnnotationUnsupported
            | Self::PmiSwiftFeatureWalkCapped
            | Self::PmiSwiftImplicitNominalUnresolved
            | Self::PmiSwiftPositionTargetUnresolved
            | Self::PmiSwiftSphericalDiameterTargetCountTextEqualRadiusAssumed
            | Self::PmiSwiftSphericalDiameterTargetUnresolved
            | Self::PmiSwiftPresentationUnresolved
            | Self::PmiSwiftInternalFlatnessSymbolAssumed
            | Self::PmiSwiftPresentationUnframed
            | Self::PmiSwiftFaceDocumentLayoutAssumed
            | Self::PmiSwiftFaceDocumentTailAssumed
            | Self::HistoryIncompleteReferences
            | Self::HistoryRollbackUnread
            | Self::HistoryStatesUnread
            | Self::FeatureIncoherentEdges
            | Self::FeatureIncoherentContent
            | Self::FeatureUnresolvedOutputScope
            | Self::FeatureIncoherentOutputs
            | Self::SketchNativeConstraint
            | Self::SketchNativeGeometry
            | Self::SketchHandleUnresolved
            | Self::SketchLineDegenerate
            | Self::SketchArcRecordConflict
            | Self::SketchMarkerRecordUnread
            | Self::SketchMarkerReadingUnbound
            | Self::SketchPointCoordinatesSubnormal
            | Self::SketchRelationPointPlaceUnsettled
            | Self::SketchPointRecordIndexBasisAssumed
            | Self::SketchArcRecordOffChord
            | Self::SketchPlaneBindingAssumed
            | Self::SketchProfileRejected
            | Self::SketchListedLineConstructionAssumed
            | Self::SketchListedCurveRelationDropped
            | Self::SketchRelationUnprojected
            | Self::SketchDimensionClassUnknown
            | Self::SketchRelationMultiplyProjected
            | Self::FeatureNativeKindRetained
            | Self::FeatureInputSolidBodyRetained
            | Self::FeatureInputObjectUnbound
            | Self::FeatureInputArchiveWalkStopped
            | Self::FeatureBlendEdgeGeneratorUnstated
            | Self::FeatureBlendMarkReferencesAssumed
            | Self::FeatureTypedOperandIncomplete
            | Self::FeatureSweepPathRetained
            | Self::FeatureSweepPathRosterAssumed
            | Self::FeatureSweepPathHeaderAssumed
            | Self::FeatureSweepMergeAssumed
            | Self::FeatureProfileCurvesUnavailable
            | Self::FeatureExtrudeDirectionUnresolved
            | Self::FeatureExtrudeReversalContradicted
            | Self::FeatureExtrudeReversalAssumed
            | Self::FeatureExtrudeStartUnresolved
            | Self::FeatureOffsetPlaneSideAssumed
            | Self::FeatureOffsetPlaneFrameConflict
            | Self::FeatureOffsetPlaneDistanceDisplayed
            | Self::FeatureRevolveAxisOffProfilePlane
            | Self::FeatureRevolveAxisRecordPrefixUnread
            | Self::FeatureThinOperationAmbiguous
            | Self::FeatureEdgeTerminalUnresolved
            | Self::FeatureBodyPathAmbiguous
            | Self::FeatureBodyRetentionUnresolved
            | Self::GeometryFaceSupportSurfaceUntyped
            | Self::GeometrySurfaceCarrierAmbiguous
            | Self::GeometrySpunProfileIntervalIgnored
            | Self::GeometryBlendCacheUnsolved
            | Self::GeometryOffsetValidityUnresolved
            | Self::GeometryEdgeSupportCurveUntyped
            | Self::GeometryTolerantEdgeCurveUnread
            | Self::GeometryPointRecordAmbiguous
            | Self::GeometryCurveMarkerConflict
            | Self::GeometryPcurveAmbiguous
            | Self::GeometrySurfaceSolverWorkCapped
            | Self::GeometrySplineLanesUnpaired
            | Self::GeometrySplineWeightNonpositive
            | Self::GeometrySplineFieldsInconsistent
            | Self::GeometryConeHalfAngleSignUnresolved
            | Self::GeometryConeHalfAngleIdentityRefused
            | Self::GeometryIntersectionLayoutUnknown
            | Self::GeometryCarrierApproximated
            | Self::AppearanceFaceColorUnresolved
            | Self::AppearanceAssignmentUnresolved
            | Self::NativeAppearanceScopeUnresolved
            | Self::TessellationFaceOwnershipUnresolved
            | Self::TessellationOwnerWorkCapped
            | Self::TessellationReusedTagAmbiguous
            | Self::TessellationDisplayWithoutGeometry
            | Self::TopologyBodyHierarchyDerived
            | Self::TopologyOwnershipWithheld
            | Self::TopologyShellRoleUnstated
            | Self::TopologyLoopChainCut
            | Self::TopologyFinRecordAmbiguous
            | Self::TopologyFinPartnerUnresolved
            | Self::TopologyLoopRecordAmbiguous
            | Self::TopologyVertexRecordUnresolved
            | Self::TopologyPcurveCarrierOffSurface
            | Self::MaterialMetadataNotTransferred
            | Self::ContainerNoncanonical
            | Self::ContainerParasolidProbeCapped
            | Self::SourceDialectUnverified
            | Self::KernelDialectUnverified
            | Self::DialectLayerCollision
            | Self::SourcePropertyKeyBlank
            | Self::MetadataPartRootUnsupported
            | Self::MetadataConfigurationManagerOpaque
            | Self::MetadataDefaultCarrierUnsupported
            | Self::MetadataDefaultOwnerAmbiguous
            | Self::MetadataDefaultAliasAmbiguous
            | Self::MetadataConfigurationPropertyOwnerAmbiguous
            | Self::MetadataConfigurationPropertyCarrierUnsupported
            | Self::MetadataConfigurationPropertyValueUnsupported
            | Self::MetadataDefaultValueUnsupported => Severity::Warning,
            Self::MetadataMassPropertiesNotCompared => Severity::Warning,
            Self::AssemblyMatesNotTransferred
            | Self::AssemblyVirtualComponentsNotTransferred
            | Self::AssemblyEmbeddedDocumentsNotTransferred
            | Self::DesignFolderDropped
            | Self::DesignStateNameNative
            | Self::DesignParameterDisplayRounded
            | Self::DesignParameterValueUnread
            | Self::DesignParameterOwnerOutsideTimeline
            | Self::DesignConfigurationActiveUnread
            | Self::DesignConfigurationStateUnplaced
            | Self::DesignConfigurationSuppressionUnread
            | Self::DesignConfigurationDefinitionUnstated
            | Self::DesignConfigurationParameterUnread
            | Self::DesignFeatureOutsideTimeline
            | Self::DesignParameterReferenceUnbound
            | Self::DesignParameterReferenceAsText
            | Self::DesignExtrudeDepthAssumed
            | Self::DesignParameterClassAssumed
            | Self::DesignParameterClassesDisagree
            | Self::DesignEquationUnread
            | Self::DesignEquationSource
            | Self::DesignSketchConstructionAssumed
            | Self::DesignExtrudeProfileDiffers
            | Self::DesignExtrudeSketchEmpty
            | Self::DesignSketchDimensionCircle
            | Self::DesignSketchLocusUnplaced
            | Self::DesignSketchLocusAmbiguous
            | Self::DesignSketchRelationSinglePoint
            | Self::DesignSketchRelationNative
            | Self::DesignSketchEntityNative
            | Self::DesignSketchDimensionUnbound
            | Self::DesignRevolveAxisDerived
            | Self::DesignRevolveProfileDiffers
            | Self::DesignRevolveSenseAssumed
            | Self::DesignDatumPlaneAxisConstructed
            | Self::DesignDatumPlaneAxisAngled
            | Self::DesignDatumConfigurationDropped => Severity::Warning,
            Self::AssemblyStructureNotTransferred => Severity::Blocking,
            Self::AssemblyPlacementUnresolved => Severity::Error,
            Self::AssemblyComponentsExternal
            | Self::AssemblySuppressedComponentsOmitted
            | Self::AssemblyConfigurationsNotTransferred
            | Self::AssemblyBomExclusionNotTransferred
            | Self::DesignFeatureUnresolved
            | Self::DesignDatumUnresolved
            | Self::SketchSecondReading
            | Self::SketchDimensionRepeated
            | Self::DesignExtrudeProfileUnchecked
            | Self::DesignRevolveAxisLineInferred
            | Self::DesignRevolveBooleanUnstated
            | Self::DesignRevolveProfileUnchecked
            | Self::HistoryRollbackMarkClassInferred
            | Self::HistoryXmlNormalised => Severity::Info,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::WriterTessellationMetadataOmitted => LossTaxonomy::AttributesNotTransferred,
            Self::BodyResolutionInvalid => LossTaxonomy::SourceTopologyInvalid,
            Self::BodyResolutionUnread => LossTaxonomy::DecodeDiagnostic,
            Self::WriterNativeEnvelopeAuxiliaryOmitted
            | Self::WriterHistoryNameNotWritten
            | Self::WriterHistoryRegenerationRefused => LossTaxonomy::MetadataNotTransferred,
            Self::WriterUnitsMemberGenerated => LossTaxonomy::MetadataNotTransferred,
            Self::WriterNativeHistorySummary => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterNativeSkeletonUnlaidRuns => LossTaxonomy::MetadataNotTransferred,
            Self::WriterNativeSkeletonOmitted => LossTaxonomy::MetadataNotTransferred,
            Self::WriterNativeStreamsOmitted => LossTaxonomy::MetadataNotTransferred,
            Self::WriterNativeSketchLaneOrphan => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterNativeBodyIdentityOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterProducerAttributesOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterNativeSketchProfileBodyKept => LossTaxonomy::MetadataNotTransferred,
            Self::WriterNativeSketchOriginRelationAdded => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterConfigurationIdentityAssigned => LossTaxonomy::MetadataNotTransferred,
            Self::WriterBodiesOmitted => LossTaxonomy::GeometryNotTransferred,
            Self::WriterFaceNamesOmitted => LossTaxonomy::AttributesNotTransferred,
            Self::WriterBodyNamesOmitted => LossTaxonomy::AttributesNotTransferred,
            Self::WriterAppearanceFieldsOmitted => LossTaxonomy::AppearanceReduced,
            Self::WriterSubdOmitted => LossTaxonomy::SubdOmitted,
            Self::WriterActiveConfigurationAssumed => LossTaxonomy::MetadataNotTransferred,
            Self::WriterFeaturesOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterFeatureResultAbsent => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterSketchConstraintsOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterEdgeRangesOmitted => LossTaxonomy::GeometryNotTransferred,
            Self::WriterPmiOmitted => LossTaxonomy::PmiOmitted,
            Self::WriterParameterExpressionsOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterCacheTolerancesOmitted => LossTaxonomy::ProceduralReduced,
            Self::WriterFaceLoopsUnchained => LossTaxonomy::TopologyNotTransferred,
            Self::WriterSelectionIdentitiesOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterFeatureFieldsOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterParametersRenamed => LossTaxonomy::AttributesNotTransferred,
            Self::WriterKeywordsCharactersReplaced => LossTaxonomy::AttributesNotTransferred,
            Self::WriterKeywordsTextTrimmed => LossTaxonomy::AttributesNotTransferred,
            Self::WriterHistoryKeywordsOnly => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterSolidStaleAfterEdit
            | Self::WriterExtrudeReversalUnwritten
            | Self::WriterExtrudeReversalConfigurationCleared => {
                LossTaxonomy::FeatureHistoryRetained
            }
            Self::WriterConfigurationSnapshotEditUnwritten => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterFeatureRecordReshaped => LossTaxonomy::FeatureHistoryRetained,
            Self::WriterReadBackDiffers => LossTaxonomy::FeatureHistoryRetained,
            Self::WriterReadBackUnread => LossTaxonomy::FeatureHistoryRetained,
            Self::WriterUnboundAppearancesOmitted => LossTaxonomy::AppearanceReduced,
            Self::WriterMaterialsOmitted => LossTaxonomy::MaterialNotTransferred,
            Self::WriterAttributesOmitted => LossTaxonomy::AttributesNotTransferred,
            Self::WriterDrivenDimensionsOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterAppearanceBindingsOmitted => LossTaxonomy::AppearanceReduced,
            Self::WriterSphereSeamCurvesOmitted => LossTaxonomy::GeometryNotTransferred,
            Self::WriterCoedgePcurveUsesOmitted => LossTaxonomy::TopologyNotTransferred,
            Self::WriterShellWireTopologyOmitted => LossTaxonomy::TopologyNotTransferred,
            Self::ContainerNoParasolidStream => LossTaxonomy::MissingGeometryStream,
            Self::SourcePreservedImageUnavailable => LossTaxonomy::PreservedSourceUnavailable,
            Self::SourceDialectUnverified | Self::KernelDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::DialectLayerCollision => LossTaxonomy::DecodeDiagnostic,
            Self::SourcePropertyKeyBlank
            | Self::MetadataPartRootUnsupported
            | Self::MetadataConfigurationManagerOpaque
            | Self::MetadataDefaultCarrierUnsupported
            | Self::MetadataDefaultOwnerAmbiguous
            | Self::MetadataDefaultAliasAmbiguous
            | Self::MetadataConfigurationPropertyOwnerAmbiguous
            | Self::MetadataConfigurationPropertyCarrierUnsupported
            | Self::MetadataConfigurationPropertyValueUnsupported
            | Self::MetadataDefaultValueUnsupported => LossTaxonomy::AttributesNotTransferred,
            Self::MetadataMassPropertiesNotCompared => LossTaxonomy::AttributesNotTransferred,
            Self::AssemblyComponentsExternal
            | Self::AssemblyVirtualComponentsNotTransferred
            | Self::AssemblyEmbeddedDocumentsNotTransferred => {
                LossTaxonomy::AssemblyComponentsExternal
            }
            Self::AssemblyStructureNotTransferred
            | Self::AssemblyMatesNotTransferred
            | Self::AssemblySuppressedComponentsOmitted
            | Self::AssemblyPlacementUnresolved
            | Self::AssemblyConfigurationsNotTransferred
            | Self::AssemblyBomExclusionNotTransferred => {
                LossTaxonomy::AssemblyPlacementsNotTransferred
            }
            Self::DesignFeatureUnresolved
            | Self::DesignDatumUnresolved
            | Self::DesignEquationUnread => LossTaxonomy::RecordNotTyped,
            Self::DesignFolderDropped => LossTaxonomy::MetadataNotTransferred,
            Self::DesignStateNameNative => LossTaxonomy::AttributesNotTransferred,
            Self::DesignParameterReferenceAsText
            | Self::DesignParameterOwnerOutsideTimeline
            | Self::DesignConfigurationActiveUnread
            | Self::DesignConfigurationStateUnplaced
            | Self::DesignConfigurationSuppressionUnread
            | Self::DesignConfigurationDefinitionUnstated
            | Self::DesignConfigurationParameterUnread
            | Self::DesignFeatureOutsideTimeline
            | Self::DesignParameterReferenceUnbound => LossTaxonomy::ParametricRecordOmitted,
            Self::DesignParameterDisplayRounded
            | Self::DesignParameterValueUnread
            | Self::DesignExtrudeDepthAssumed
            | Self::DesignParameterClassAssumed
            | Self::DesignParameterClassesDisagree
            | Self::DesignEquationSource
            | Self::DesignSketchConstructionAssumed
            | Self::DesignExtrudeProfileDiffers
            | Self::DesignExtrudeSketchEmpty
            | Self::DesignExtrudeProfileUnchecked
            | Self::DesignSketchDimensionCircle
            | Self::DesignSketchLocusUnplaced
            | Self::DesignSketchLocusAmbiguous
            | Self::DesignSketchRelationSinglePoint
            | Self::DesignSketchRelationNative
            | Self::DesignSketchEntityNative
            | Self::DesignSketchDimensionUnbound
            | Self::DesignRevolveAxisLineInferred
            | Self::DesignRevolveAxisDerived
            | Self::DesignRevolveBooleanUnstated
            | Self::DesignRevolveProfileDiffers
            | Self::DesignRevolveProfileUnchecked
            | Self::DesignRevolveSenseAssumed
            | Self::DesignDatumPlaneAxisConstructed
            | Self::DesignDatumPlaneAxisAngled
            | Self::DesignDatumConfigurationDropped => LossTaxonomy::DecodeDiagnostic,
            Self::TopologyBodyHierarchyDerived => LossTaxonomy::TopologyGaugeSubstituted,
            Self::TopologyOwnershipWithheld
            | Self::TopologyShellRoleUnstated
            | Self::TopologyLoopChainCut
            | Self::TopologyFinRecordAmbiguous
            | Self::TopologyFinPartnerUnresolved
            | Self::TopologyLoopRecordAmbiguous
            | Self::TopologyVertexRecordUnresolved => LossTaxonomy::TopologyNotTransferred,
            Self::TopologyPcurveCarrierOffSurface => LossTaxonomy::PcurveOmitted,
            Self::GeometryCarrierApproximated => LossTaxonomy::ProceduralReduced,
            Self::GeometryBlendCacheUnsolved => LossTaxonomy::DecodeDiagnostic,
            Self::TopologyGraphNotTransferred => LossTaxonomy::TopologyNotTransferred,
            Self::GeometryFaceSupportSurfaceUntyped
            | Self::GeometrySurfaceCarrierAmbiguous
            | Self::GeometrySpunProfileIntervalIgnored
            | Self::GeometryOffsetValidityUnresolved
            | Self::GeometryEdgeSupportCurveUntyped
            | Self::GeometryTolerantEdgeCurveUnread
            | Self::GeometryPointRecordAmbiguous
            | Self::GeometryCurveMarkerConflict
            | Self::GeometrySplineLanesUnpaired
            | Self::GeometrySplineWeightNonpositive
            | Self::GeometrySplineFieldsInconsistent
            | Self::GeometryConeHalfAngleSignUnresolved
            | Self::GeometryConeHalfAngleIdentityRefused
            | Self::GeometryIntersectionLayoutUnknown
            | Self::GeometryParasolidNotTransferred
            | Self::GeometryMeshBodyNotTransferred
            | Self::GeometryImportedBodiesNotTransferred
            | Self::ContainerParasolidProbeCapped => LossTaxonomy::GeometryNotTransferred,
            Self::GeometryPcurveAmbiguous | Self::GeometrySurfaceSolverWorkCapped => {
                LossTaxonomy::PcurveOmitted
            }
            Self::AppearanceFaceColorUnresolved
            | Self::AppearanceAssignmentUnresolved
            | Self::NativeAppearanceScopeUnresolved => LossTaxonomy::MaterialNotTransferred,
            Self::TessellationFaceOwnershipUnresolved
            | Self::TessellationOwnerWorkCapped
            | Self::TessellationReusedTagAmbiguous
            | Self::TessellationDisplayWithoutGeometry => LossTaxonomy::ReferenceGraphNotClosed,
            Self::MaterialMetadataNotTransferred => LossTaxonomy::MaterialNotTransferred,
            Self::PmiSwiftImplicitNominalUnresolved
            | Self::PmiSwiftPositionTargetUnresolved
            | Self::PmiSwiftSphericalDiameterTargetCountTextEqualRadiusAssumed
            | Self::PmiSwiftSphericalDiameterTargetUnresolved
            | Self::PmiSwiftPresentationUnresolved
            | Self::PmiSwiftInternalFlatnessSymbolAssumed
            | Self::PmiSwiftPresentationUnframed
            | Self::PmiSwiftFaceDocumentLayoutAssumed
            | Self::PmiSwiftFaceDocumentTailAssumed
            | Self::ContainerNoncanonical
            | Self::HistoryRollbackMarkClassInferred => LossTaxonomy::DecodeDiagnostic,
            Self::HistoryApplicationNotTransferred
            | Self::HistoryApplicationNestingCut
            | Self::HistoryXmlNormalised => LossTaxonomy::MetadataNotTransferred,
            Self::ConfigActiveIdentityUnresolved
            | Self::ConfigActivePartitionMismatch
            | Self::ConfigActivePartitionUnread
            | Self::ConfigInferredWithoutNative
            | Self::ConfigLaneIdentityUnresolved
            | Self::ConfigAmbiguousPartition
            | Self::ConfigAmbiguousNaming
            | Self::ConfigIncoherentBodyRefs
            | Self::ConfigIncompleteSnapshot
            | Self::ConfigProjectionWorkCapped
            | Self::ConfigStateUnstated
            | Self::ConfigStateNotStored
            | Self::ConfigStatePresentationUnstated
            | Self::ConfigStateImportedBodiesNotTransferred
            | Self::ConfigStateOutputsUnstated
            | Self::ParameterUnevaluated
            | Self::ParameterAmbiguousIdentity
            | Self::PmiDimensionUnbound
            | Self::PmiSemanticParseCapped
            | Self::PmiSwiftAnnotationUnsupported
            | Self::PmiSwiftFeatureWalkCapped
            | Self::HistoryIncompleteReferences
            | Self::HistoryRollbackUnread
            | Self::HistoryStatesUnread
            | Self::FeatureIncoherentEdges
            | Self::FeatureIncoherentContent
            | Self::FeatureUnresolvedOutputScope
            | Self::FeatureIncoherentOutputs
            | Self::SketchNativeConstraint
            | Self::SketchNativeGeometry
            | Self::SketchHandleUnresolved
            | Self::SketchLineDegenerate
            | Self::SketchArcRecordConflict
            | Self::SketchMarkerRecordUnread
            | Self::SketchSecondReading
            | Self::SketchMarkerReadingUnbound
            | Self::SketchPointCoordinatesSubnormal
            | Self::SketchRelationPointPlaceUnsettled
            | Self::SketchPointRecordIndexBasisAssumed
            | Self::SketchArcRecordOffChord
            | Self::SketchPlaneBindingAssumed
            | Self::SketchProfileRejected
            | Self::SketchListedLineConstructionAssumed
            | Self::SketchListedCurveRelationDropped
            | Self::SketchRelationUnprojected
            | Self::SketchDimensionClassUnknown
            | Self::SketchDimensionRepeated
            | Self::SketchRelationMultiplyProjected
            | Self::FeatureNativeKindRetained
            | Self::FeatureInputSolidBodyRetained
            | Self::FeatureInputObjectUnbound
            | Self::FeatureInputArchiveWalkStopped
            | Self::FeatureBlendEdgeGeneratorUnstated
            | Self::FeatureBlendMarkReferencesAssumed
            | Self::FeatureTypedOperandIncomplete
            | Self::FeatureSweepPathRetained
            | Self::FeatureSweepPathRosterAssumed
            | Self::FeatureSweepPathHeaderAssumed
            | Self::FeatureSweepMergeAssumed
            | Self::FeatureProfileCurvesUnavailable
            | Self::FeatureExtrudeDirectionUnresolved
            | Self::FeatureExtrudeReversalContradicted
            | Self::FeatureExtrudeReversalAssumed
            | Self::FeatureExtrudeStartUnresolved
            | Self::FeatureOffsetPlaneSideAssumed
            | Self::FeatureOffsetPlaneFrameConflict
            | Self::FeatureOffsetPlaneDistanceDisplayed
            | Self::FeatureRevolveAxisOffProfilePlane
            | Self::FeatureRevolveAxisRecordPrefixUnread
            | Self::FeatureThinOperationAmbiguous
            | Self::FeatureEdgeTerminalUnresolved
            | Self::FeatureBodyPathAmbiguous
            | Self::FeatureBodyRetentionUnresolved => LossTaxonomy::FeatureHistoryRetained,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        NamespacedLossKind::new(
            const {
                match LossNamespace::new("sldprt") {
                    Ok(namespace) => namespace,
                    Err(_) => panic!("reserved codec namespace"),
                }
            },
            self.code(),
            self.shared_taxonomy(),
        )
        .with_strict_floor(self.strict_floor())
        .into()
    }

    /// The severity floor strict mode applies to this code (CADIR §3.2).
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::ContainerNoncanonical => Some(Severity::Warning),
            _ => None,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `sldprt/<local>`. Severity comes from the local
    /// code.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

/// Instances an aggregated loss note names before it only counts the rest.
pub(crate) const NAMED_INSTANCES_PER_NOTE: usize = 8;

/// A count of affected instances that keeps the first
/// [`NAMED_INSTANCES_PER_NOTE`] names for one aggregated loss note.
#[derive(Debug, Default, Clone)]
pub(crate) struct NamedCount {
    count: usize,
    named: Vec<String>,
}

impl NamedCount {
    pub(crate) fn push(&mut self, name: &str) {
        self.count += 1;
        if self.named.len() < NAMED_INSTANCES_PER_NOTE {
            self.named.push(name.to_owned());
        }
    }

    pub(crate) fn append(&mut self, other: &mut Self) {
        self.count += other.count;
        let room = NAMED_INSTANCES_PER_NOTE - self.named.len().min(NAMED_INSTANCES_PER_NOTE);
        self.named.extend(other.named.drain(..).take(room));
        other.count = 0;
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl std::fmt::Display for NamedCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.named.join(", "))?;
        if self.count > self.named.len() {
            f.write_str(", ...")?;
        }
        Ok(())
    }
}

/// One refused spline lane, named by the record that stated it.
///
/// `record` names the instance: the attribute id, coedge id, edge id or face
/// attribute of the record whose lanes do not pair, followed by the pairing's
/// own refusal. The B-rep model carries the absence of one carrier, so the
/// refusal is a loss and the decode continues.
pub(crate) fn spline_lane_refusal(record: impl std::fmt::Display) -> LossNote {
    let message = format!(
        "{record}; the carrier is not emitted and the entities that reference it fall back \
     to an untyped support."
    );
    SldprtLossCode::GeometrySplineLanesUnpaired.note(message)
}

/// One refused spline carrier, named by `record`, with the loss code and
/// message of its refusal.
pub(crate) fn spline_refusal(
    record: impl std::fmt::Display,
    refusal: &cadmpeg_parasolid::geometry::nurbs::Refusal,
) -> LossNote {
    use cadmpeg_parasolid::geometry::nurbs::Refusal;
    let (code, cause) = match refusal {
        Refusal::Lanes(error) => return spline_lane_refusal(format_args!("{record}: {error}")),
        Refusal::NonpositiveWeight => (
            SldprtLossCode::GeometrySplineWeightNonpositive,
            "stores a control-point weight that is zero or negative",
        ),
        Refusal::Fields => (
            SldprtLossCode::GeometrySplineFieldsInconsistent,
            "stores a dimension, count, or multiplicity that contradicts its fields or arrays",
        ),
    };
    code.note(format!(
        "{record} {cause}; the carrier is not emitted and the entities that reference it fall \
         back to an untyped support."
    ))
}

/// Number of names that one loss message lists before it counts the rest.
const NAMED_PER_NOTE: usize = 8;

/// The first [`NAMED_PER_NOTE`] of `names`, then the count of the rest.
pub(crate) fn named<S: AsRef<str>>(names: &[S]) -> String {
    let mut text = names
        .iter()
        .take(NAMED_PER_NOTE)
        .map(|name| format!("`{}`", name.as_ref()))
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > NAMED_PER_NOTE {
        text = format!("{text}, and {} more", names.len() - NAMED_PER_NOTE);
    }
    text
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::report::loss::LossTaxonomy;
    use cadmpeg_ir::report::Severity;

    use crate::test_support::ir::strict_options;

    use super::SldprtLossCode;
    use std::collections::BTreeSet;

    #[test]
    fn named_count_names_the_first_instances_and_counts_the_rest() {
        let mut first = super::NamedCount::default();
        for index in 0..super::NAMED_INSTANCES_PER_NOTE - 1 {
            first.push(&format!("a{index}"));
        }
        let mut second = super::NamedCount::default();
        second.push("b0");
        second.push("b1");
        first.append(&mut second);
        assert_eq!(first.count(), super::NAMED_INSTANCES_PER_NOTE + 1);
        assert!(second.is_empty());
        assert_eq!(first.to_string(), "a0, a1, a2, a3, a4, a5, a6, b0, ...");
    }

    #[test]
    fn spline_lane_loss_names_the_record() {
        let note = super::spline_lane_refusal("carrier attribute 9");
        assert_eq!(
            note.message,
            "carrier attribute 9; the carrier is not emitted and the entities that reference it fall back to an untyped support."
        );
    }

    #[test]
    fn all_covers_every_declared_variant() {
        let source = include_str!("loss.rs");
        let declaration = source
            .split_once("pub(crate) enum SldprtLossCode {")
            .unwrap()
            .1;
        let body = declaration.split_once("\n}").unwrap().0;
        let variant_count = body
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with("//") && line.ends_with(','))
            .count();
        assert_eq!(SldprtLossCode::ALL.len(), variant_count);
    }

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = SldprtLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "metadata.part-root-unsupported",
                "metadata.configuration-manager-opaque",
                "metadata.default-carrier-unsupported",
                "metadata.default-owner-ambiguous",
                "metadata.default-alias-ambiguous",
                "metadata.default-value-unsupported",
                "metadata.mass-properties-not-compared",
                "metadata.configuration-property-owner-ambiguous",
                "metadata.configuration-property-carrier-unsupported",
                "metadata.configuration-property-value-unsupported",
                "config.active-identity-unresolved",
                "config.active-partition-mismatch",
                "config.active-partition-unread",
                "config.inferred-without-native",
                "config.lane-identity-unresolved",
                "config.ambiguous-partition",
                "config.ambiguous-naming",
                "config.incoherent-body-refs",
                "config.incomplete-snapshot",
                "config.projection-work-capped",
                "config.state-unstated",
                "config.state-not-stored",
                "config.state-presentation-unstated",
                "config.state-imported-bodies-not-transferred",
                "config.state-outputs-unstated",
                "parameter.unevaluated",
                "parameter.ambiguous-identity",
                "pmi.dimension-unbound",
                "pmi.semantic-parse-capped",
                "pmi.swift-annotation-unsupported",
                "pmi.swift-feature-walk-capped",
                "pmi.swift-implicit-nominal-unresolved",
                "pmi.swift-position-target-unresolved",
                "pmi.swift-spherical-diameter-count-text-equal-radius-assumed",
                "pmi.swift-spherical-diameter-target-unresolved",
                "pmi.swift-presentation-unresolved",
                "pmi.swift-internal-flatness-symbol-assumed",
                "pmi.swift-presentation-unframed",
                "pmi.swift-face-document-layout-assumed",
                "pmi.swift-face-document-tail-assumed",
                "history.incomplete-references",
                "history.rollback-unread",
                "history.states-unread",
                "history.rollback-mark-class-inferred",
                "history.application-not-transferred",
                "history.application-nesting-cut",
                "history.xml-normalised",
                "feature.incoherent-edges",
                "feature.incoherent-content",
                "feature.unresolved-output-scope",
                "feature.incoherent-outputs",
                "sketch.native-constraint",
                "sketch.native-geometry",
                "sketch.handle-unresolved",
                "sketch.line-degenerate",
                "sketch.arc-record-conflict",
                "sketch.marker-record-unread",
                "sketch.second-reading",
                "sketch.marker-reading-unbound",
                "sketch.point-coordinates-subnormal",
                "sketch.relation-point-place-unsettled",
                "sketch.plane-binding-assumed",
                "sketch.point-record-index-basis-assumed",
                "sketch.arc-record-off-chord",
                "sketch.profile-rejected",
                "sketch.listed-line-construction-assumed",
                "sketch.listed-curve-relation-dropped",
                "sketch.relation-unprojected",
                "sketch.relation-multiply-projected",
                "sketch.dimension-class-unknown",
                "sketch.dimension-repeated",
                "feature.native-kind-retained",
                "feature.input-object-unbound",
                "feature.input-solid-body-retained",
                "feature.input-archive-walk-stopped",
                "feature.blend-edge-generator-unstated",
                "feature.blend-mark-references-assumed",
                "feature.typed-operand-incomplete",
                "feature.sweep-path-retained",
                "feature.sweep-path-roster-assumed",
                "feature.sweep-path-header-assumed",
                "feature.sweep-merge-assumed",
                "feature.profile-curves-unavailable",
                "feature.extrude-direction-unresolved",
                "feature.extrude-reversal-contradicted",
                "feature.extrude-reversal-assumed",
                "feature.extrude-start-unresolved",
                "feature.offset-plane-side-assumed",
                "feature.offset-plane-frame-conflict",
                "feature.offset-plane-distance-displayed",
                "feature.revolve-axis-off-profile-plane",
                "feature.revolve-axis-record-prefix-unread",
                "feature.thin-operation-ambiguous",
                "feature.edge-terminal-unresolved",
                "feature.body-path-ambiguous",
                "feature.body-retention-unresolved",
                "geometry.face-support-surface-untyped",
                "geometry.surface-carrier-ambiguous",
                "geometry.offset-validity-unresolved",
                "geometry.spun-profile-interval-ignored",
                "geometry.blend-cache-unsolved",
                "geometry.edge-support-curve-untyped",
                "geometry.tolerant-edge-curve-unread",
                "geometry.point-record-ambiguous",
                "geometry.curve-marker-conflict",
                "geometry.pcurve-ambiguous",
                "geometry.surface-solver-work-capped",
                "geometry.spline-lanes-unpaired",
                "geometry.spline-weight-nonpositive",
                "geometry.spline-fields-inconsistent",
                "geometry.cone-half-angle-sign-unresolved",
                "geometry.cone-half-angle-identity-refused",
                "geometry.intersection-layout-unknown",
                "geometry.carrier-approximated",
                "appearance.face-color-unresolved",
                "appearance.assignment-unresolved",
                "appearance.native-scope-unresolved",
                "tessellation.face-ownership-unresolved",
                "tessellation.owner-work-capped",
                "tessellation.reused-tag-ambiguous",
                "tessellation.display-without-geometry",
                "topology.body-hierarchy-derived",
                "topology.loop-chain-cut",
                "topology.ownership-withheld",
                "topology.shell-role-unstated",
                "topology.fin-record-ambiguous",
                "topology.fin-partner-unresolved",
                "topology.loop-record-ambiguous",
                "topology.vertex-record-unresolved",
                "topology.pcurve-carrier-off-surface",
                "geometry.parasolid-not-transferred",
                "geometry.mesh-body-not-transferred",
                "geometry.imported-bodies-not-transferred",
                "body.resolution-invalid",
                "body.resolution-unread",
                "topology.graph-not-transferred",
                "material.metadata-not-transferred",
                "container.no-parasolid-stream",
                "container.parasolid-probe-capped",
                "container.noncanonical",
                "source.preserved-image-unavailable",
                "source.dialect-unverified",
                "source.kernel-dialect-unverified",
                "source.dialect-layer-collision",
                "writer.shell-wire-topology-omitted",
                "writer.coedge-pcurve-uses-omitted",
                "writer.sphere-seam-curves-omitted",
                "writer.appearance-bindings-omitted",
                "writer.attributes-omitted",
                "writer.driven-dimensions-omitted",
                "writer.unbound-appearances-omitted",
                "writer.materials-omitted",
                "writer.tessellation-metadata-omitted",
                "writer.native-envelope-auxiliary-omitted",
                "writer.units-member-generated",
                "writer.native-history-summary",
                "writer.native-skeleton-unlaid-runs",
                "writer.native-skeleton-omitted",
                "writer.native-streams-omitted",
                "writer.native-sketch-lane-orphan",
                "writer.native-body-identity-omitted",
                "writer.producer-attributes-omitted",
                "writer.native-sketch-profile-body-kept",
                "writer.native-sketch-origin-relation-added",
                "writer.configuration-identity-assigned",
                "writer.bodies-omitted",
                "writer.face-names-omitted",
                "writer.body-names-omitted",
                "writer.appearance-fields-omitted",
                "writer.subd-omitted",
                "writer.active-configuration-assumed",
                "writer.features-omitted",
                "writer.feature-result-absent",
                "writer.sketch-constraints-omitted",
                "writer.edge-ranges-omitted",
                "writer.pmi-omitted",
                "writer.parameter-expressions-omitted",
                "writer.cache-tolerances-omitted",
                "writer.face-loops-unchained",
                "writer.selection-identities-omitted",
                "writer.feature-fields-omitted",
                "writer.parameters-renamed",
                "writer.keywords-characters-replaced",
                "writer.keywords-text-trimmed",
                "writer.history-keywords-only",
                "writer.history-name-not-written",
                "writer.history-regeneration-refused",
                "writer.solid-stale-after-edit",
                "writer.configuration-snapshot-edit-unwritten",
                "writer.feature-record-reshaped",
                "writer.read-back-differs",
                "writer.read-back-unread",
                "writer.extrude-reversal-unwritten",
                "writer.extrude-reversal-configuration-cleared",
                "source.property-key-blank",
                "assembly.structure-not-transferred",
                "assembly.mates-not-transferred",
                "assembly.components-external",
                "assembly.virtual-components-not-transferred",
                "assembly.suppressed-components-omitted",
                "assembly.placement-unresolved",
                "assembly.configurations-not-transferred",
                "assembly.embedded-documents-not-transferred",
                "assembly.bom-exclusion-not-transferred",
                "design.feature-unresolved",
                "design.datum-unresolved",
                "design.folder-dropped",
                "design.state-name-native",
                "design.parameter-display-rounded",
                "design.parameter-value-unread",
                "design.parameter-owner-outside-timeline",
                "design.configuration-active-unread",
                "design.configuration-state-unplaced",
                "design.configuration-suppression-unread",
                "design.configuration-definition-unstated",
                "design.configuration-parameter-unread",
                "design.feature-outside-timeline",
                "design.parameter-reference-unbound",
                "design.parameter-reference-as-text",
                "design.extrude-depth-assumed",
                "design.parameter-class-assumed",
                "design.parameter-classes-disagree",
                "design.equation-unread",
                "design.equation-source",
                "design.sketch-construction-assumed",
                "design.extrude-profile-differs",
                "design.extrude-sketch-empty",
                "design.extrude-profile-unchecked",
                "design.sketch-dimension-circle",
                "design.sketch-locus-unplaced",
                "design.sketch-locus-ambiguous",
                "design.sketch-relation-single-point",
                "design.sketch-relation-native",
                "design.sketch-entity-native",
                "design.sketch-dimension-unbound",
                "design.revolve-axis-line-inferred",
                "design.revolve-axis-derived",
                "design.revolve-boolean-unstated",
                "design.revolve-profile-differs",
                "design.revolve-profile-unchecked",
                "design.revolve-sense-assumed",
                "design.datum-plane-axis-constructed",
                "design.datum-plane-axis-angled",
                "design.datum-configuration-dropped",
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

    #[test]
    fn the_compound_deviation_alone_pins_a_strict_floor() {
        for code in SldprtLossCode::ALL {
            let expected =
                (*code == SldprtLossCode::ContainerNoncanonical).then_some(Severity::Warning);
            assert_eq!(code.note("").code.strict_floor(), expected, "{code:?}");
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
    fn implicit_swift_nominal_is_a_tolerable_decode_diagnostic() {
        let note = SldprtLossCode::PmiSwiftImplicitNominalUnresolved.note("unresolved");
        assert_eq!(note.severity, Severity::Warning);
        assert_eq!(note.code.taxonomy(), LossTaxonomy::DecodeDiagnostic);
        assert!(!strict_options().strictness.refuses(&note, false));
    }

    #[test]
    fn unresolved_position_target_is_a_tolerable_decode_diagnostic() {
        let note = SldprtLossCode::PmiSwiftPositionTargetUnresolved.note("unresolved");
        assert_eq!(note.severity, Severity::Warning);
        assert_eq!(note.code.taxonomy(), LossTaxonomy::DecodeDiagnostic);
        assert!(!strict_options().strictness.refuses(&note, false));
    }

    #[test]
    fn ambiguous_pcurve_is_a_tolerable_pcurve_omission() {
        let note = SldprtLossCode::GeometryPcurveAmbiguous.note("ambiguous");
        assert_eq!(note.code.namespace(), "sldprt");
        assert_eq!(note.code.local_code(), "geometry.pcurve-ambiguous");
        assert_eq!(note.code.taxonomy(), LossTaxonomy::PcurveOmitted);
        assert!(!strict_options().strictness.refuses(&note, false));
    }
}
