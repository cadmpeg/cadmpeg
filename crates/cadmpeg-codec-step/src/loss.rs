// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for STEP / `.stp` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`StepLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`StepLossCode::note`] is the single construction path for a
//! [`LossNote`] in this crate: it fixes the shared loss category and the
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `step` namespace.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy},
    Severity,
};

const NAMESPACE: LossNamespace<'static> = match LossNamespace::new("step") {
    Ok(namespace) => namespace,
    Err(_) => panic!("reserved codec namespace"),
};

/// A stable, machine-readable identifier for one STEP / `.stp` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`StepLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum StepLossCode {
    /// Parser recovered noncanonical Part 21 syntax.
    ParseNoncanonicalSyntax,
    /// A bounded DATA record or its dependent could not be interpreted.
    ParseRecordOmitted,
    /// A bounded noncanonical prefix remains source-only.
    ParsePreambleOmitted,
    /// A pre-standard exchange used the Part 21 instance grammar.
    ParseDraftGrammar,
    /// Readable DATA ends before a complete closing envelope.
    ParseEnvelopeIncomplete,
    /// ZIP metadata violates the Part 21 profile without preventing root decoding.
    ContainerMemberNoncanonical,
    /// A decode stage surfaced a per-record warning.
    DecodeWarning,
    /// Byte accounting left source bytes unclassified.
    ByteAccountingUnclassified,
    /// Named entity instances were retained as opaque records.
    OpaqueRecordPreserved,
    /// A metadata string field failed to decode.
    MetadataStringInvalid,
    /// A `FILE_SCHEMA` object identifier component is out of range.
    SchemaObjectIdentifierOutOfRange,
    /// The declared Part 21 implementation level has no implemented grammar.
    ImplementationLevelUnverified,
    /// Header metadata deviates from the format without preventing decoding.
    HeaderMetadataNoncanonical,
    /// The declared `FILE_SCHEMA` identifier satisfies no declared dialect.
    SourceDialectUnverified,
    /// The declared schema object identifier names a module edition that the
    /// decoder reads as a published AP edition.
    SourceSchemaEditionAssumed,
    /// An attribute string field failed to decode.
    AttributeStringInvalid,
    /// `AXIS2_PLACEMENT_3D` reference direction was parallel to its axis.
    PlacementReferenceInferred,
    /// Geometry records belong to representations with conflicting units.
    ConflictingRepresentationUnits,
    /// Representation contexts state different length uncertainties, and a
    /// value reaches no topology or a measure does not resolve.
    UncertaintyLengthAmbiguous,
    /// `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` has no resolvable length measure.
    UncertaintyLengthUnresolved,
    /// The document length unit did not resolve.
    DocumentLengthUnitUnresolved,
    /// The document plane-angle unit did not resolve.
    DocumentAngleUnitUnresolved,
    /// A LINE parameter scale did not resolve.
    LineParameterScaleUnresolved,
    /// A topology root was rejected with a failure message.
    TopologyRootRejected,
    /// A topology root does not resolve to a complete connected graph.
    TopologyRootIncomplete,
    /// An oriented shell omits the derived `cfs_faces` slot.
    OrientedShellOmitsCfsFaces,
    /// A `SEAM_EDGE` has no decoded pcurve on its edge curve and face surface.
    SeamEdgePcurveUnresolved,
    /// An edge has no decoded surface or curve carrier for a pcurve.
    EdgeNoSurfaceOrCurveForPcurve,
    /// A single optional pcurve is not endpoint-continuous with the edge.
    PcurveEndpointsDiscontinuous,
    /// A single endpoint-continuous pcurve fails the bounded model-space locus witness.
    PcurveLocusDiscontinuous,
    /// A finite non-seam pcurve witness did not prove global point-set and direction fidelity.
    PcurveGlobalFidelityUnproved,
    /// Several pcurves associate with a surface and none selects uniquely.
    PcurveAssociationAmbiguous,
    /// Pcurve candidates exist but the source surface or curve is unresolved.
    PcurveCandidatesCarrierUnresolved,
    /// A face has more than one `FACE_OUTER_BOUND`.
    FaceMultipleOuterBounds,
    /// A source shell contains disconnected face components.
    ShellDisconnectedFaces,
    /// A NAUO has no resolved occurrence transform.
    NauoPlacementUnresolved,
    /// A NAUO has competing resolved placement candidates.
    NauoPlacementAmbiguous,
    /// A body has conflicting standalone `MAPPED_ITEM` placements.
    BodyConflictingMappedPlacements,
    /// A presentation annotation has several text carriers and no order.
    PresentationAnnotationTextUnordered,
    /// A presentation annotation has several placement carriers and no unique placement.
    PresentationAnnotationPlacementAmbiguous,
    /// A dimensional characteristic has several named nominal measures.
    DimensionalNominalAmbiguous,
    /// A dimensional characteristic has several unnamed measure values.
    DimensionalUnnamedMeasureAmbiguous,
    /// A PMI length measure unit scale did not resolve.
    PmiLengthUnitUnresolved,
    /// A PMI angle measure unit scale did not resolve.
    PmiAngleUnitUnresolved,
    /// A datum system has inconsistent precedence compartments.
    PmiDatumSystemInvalid,
    /// A tolerance-zone constraint has no complete semantic interpretation.
    PmiZoneUnresolved,
    /// A dimensional modifier has no complete semantic value.
    PmiDimensionModifierUnresolved,
    /// A dimension's nominal or limit quantity disagrees with the kind of its
    /// dimension entity, so the dimension keeps the entity name as its kind.
    PmiDimensionKindQuantityMismatch,
    /// Dimensional limit measures do not form one lower and one upper limit.
    DimensionalLimitsOmitted,
    /// Independent styled items assign conflicting scalar colors.
    ConflictingScalarColors,
    /// A surface style usage has an invalid `surface_side` enumeration value.
    SurfaceSideInvalid,
    /// A surface rendering has multiple transparency properties.
    SurfaceTransparencyConflict,
    /// A surface side style lists a transparency outside a rendering.
    SurfaceTransparencyMisplaced,
    /// A context-dependent style has no matching neutral presentation context.
    ContextDependentStyleUnresolved,
    /// A drawing record has too few parameters and was retained opaque.
    DrawingRecordTooFewParameters,
    /// A drawing's position in the stored order passes the stated order width.
    DrawingOrderUnstatable,
    /// A drawing relationship references a source-typed record without identity.
    DrawingRelationshipUntypedTarget,
    /// A drawing relationship references a source record with multiple identities.
    DrawingRelationshipTargetAmbiguous,
    /// A drawing sheet usage has no resolvable drawing revision.
    DrawingSheetRevisionUnresolved,
    /// A drawing revision usage has no resolvable sheet revision.
    DrawingRevisionSheetUnresolved,
    /// A draughting model association references a typed semantic definition without identity.
    DraughtingSemanticDefinitionUntyped,
    /// A draughting model association references a source-typed item without identity.
    DraughtingAssociatedItemUntyped,
    /// A body-representation tessellation item does not bind to exactly one decoded body.
    TessellationItemBodyUnresolved,
    /// A tessellation payload fails numeric or structural admission.
    TessellationInvalidPayload,
    /// A tessellation item lacks an exact body-container or tessellated-representation declaration.
    TessellationItemUndeclared,
    /// A repositioned tessellation item has no valid placement.
    TessellationPlacementUnresolved,
    /// A detached tessellation item has multiple distinct repositioning placements.
    TessellationPlacementAmbiguous,
    /// A geometric validation measure unit scale did not resolve.
    ValidationMeasureUnitUnresolved,
    /// A geometric validation item states no quantity that a declared measure carries.
    ValidationItemUnsupported,
    /// The IR document has bodies but no exportable solids.
    NoExportableSolids,
    /// A layer item has no writable STEP carrier.
    LayerItemWithoutCarrier,
    /// A sublayer is written as a top-level layer.
    LayerHierarchyFlattened,
    /// An occurrence has an unresolved parent.
    OccurrenceUnresolvedParent,
    /// An occurrence has no local product definition.
    OccurrenceNoLocalProduct,
    /// An occurrence placement is not rigid.
    OccurrencePlacementNotRigid,
    /// A body carries a non-rigid transform.
    BodyNonRigidTransform,
    /// Hidden body visibility assignments are unsupported by the target schema.
    HiddenBodyVisibilityUnsupported,
    /// Hidden appearance binding visibility assignments are unsupported by the target schema.
    HiddenAppearanceVisibilityUnsupported,
    /// Hidden presentation layer visibility assignments are unsupported by the target schema.
    HiddenPresentationLayerVisibilityUnsupported,
    /// Hidden PMI annotation visibility assignments are unsupported by the target schema.
    HiddenPmiVisibilityUnsupported,
    /// A wire shell has free vertices without an edge-based STEP carrier.
    WireShellFreeVertices,
    /// Bodies without component ownership are assigned to the root product.
    BodyWithoutProductRepresentation,
    /// Solid and sheet shells do not retain wire members within the shell.
    SurfaceShellWireTopologyOmitted,
    /// Tessellations require an AP242 target.
    TessellationRequiresAp242,
    /// Tessellation feature-edge classification is not represented.
    TessellationFeatureEdges,
    /// Tessellation corner normals are not represented.
    TessellationCornerNormals,
    /// Tessellation triangle groups are not represented.
    TessellationTriangleGroups,
    /// Tessellation texture assignments are not represented.
    TessellationTextureAssignments,
    /// A tessellation has invalid vertex, index, or normal cardinality.
    TessellationInvalidCardinality,
    /// A tessellation body has no writable AP242 tessellation link.
    TessellationBodyLinkUnwritable,
    /// Tessellation metadata was reduced because STEP cannot represent it.
    TessellationMetadataReduced,
    /// Topology is not reachable from any emitted region shape item.
    TopologyUnreachableFromRegion,
    /// Signed or self-intersecting analytic surfaces were normalized.
    AnalyticSurfaceNormalized,
    /// Elliptical cone surfaces were reduced to circular `CONICAL_SURFACE`.
    EllipticalConeReduced,
    /// Edges have no typed 3D curve or carry an unsupported transform.
    CurvelessEdgeOmitted,
    /// Faces rest on an unknown or STEP-unsupported surface.
    UnknownSurfaceFaceOmitted,
    /// Geometry carriers were not written.
    GeometryCarrierNotWritten,
    /// Coedge pcurve carriers use unwritable geometry or surface references.
    PcurveCarrierUnwritable,
    /// Pcurves on helix surfaces written as fitted B-spline surfaces, whose
    /// parameters differ from the native ones.
    PcurveOnFittedHelixSurface,
    /// Assembly occurrences were omitted because the parent has no local product.
    AssemblyOccurrenceOmittedNoParentProduct,
    /// Regions have no shell list and were not written.
    RegionNoShellList,
    /// Wire regions had no writable connected edge set.
    WireRegionNoConnectedEdgeSet,
    /// Wire region or shell relations referenced missing shell records.
    WireRegionMissingShell,
    /// Hidden bodies had no emitted STEP item and were omitted from INVISIBILITY.
    HiddenBodyOmitted,
    /// Hidden presentation layers had no emitted STEP assignment and were omitted from INVISIBILITY.
    HiddenPresentationLayerOmitted,
    /// Appearance bindings reference missing appearance assets.
    AppearanceBindingMissingAsset,
    /// Appearance bindings reference appearances without a base color.
    AppearanceBindingNoBaseColor,
    /// Appearance bindings target one body or face with conflicting styles.
    AppearanceBindingTargetConflict,
    /// Coedge pcurve references have no geometry.
    CoedgePcurveNoGeometry,
    /// Emitted coedge pcurves carry native-only metadata.
    CoedgePcurveNativeMetadata,
    /// Pcurve uses carry native-only parameter metadata.
    PcurveUseNativeMetadata,
    /// Coedge-local 3D curve uses were not represented.
    CoedgeUseCurveNotRepresented,
    /// Topology metadata values were not represented.
    TopologyMetadataNotRepresented,
    /// Subdivision surfaces were omitted.
    SubdOmitted,
    /// Parametric or design records were not represented.
    ParametricDesignRecordsOmitted,
    /// Drawing or presentation records were not represented.
    DrawingPresentationRecordsOmitted,
    /// Semantic annotations were not represented.
    SemanticAnnotationOmitted,
    /// Document assets were not represented.
    DocumentAssetOmitted,
    /// Assembly joints were not represented.
    AssemblyJointOmitted,
    /// Product definitions use a non-part kind.
    ProductNonPartKind,
    /// Product BOM property values were not represented.
    ProductBomPropertyOmitted,
    /// Occurrences reference external product definitions.
    OccurrenceExternalProduct,
    /// Occurrences have no writable product definition.
    OccurrenceNoWritableProduct,
    /// Occurrence metadata values were not represented.
    OccurrenceMetadataOmitted,
    /// An instance that shows its own bodies or nested placements was
    /// written as an occurrence of a product definition of its own.
    InstanceDefinitionSplit,
    /// An instance path names no occurrence of the document.
    InstancePathUnresolved,
    /// A definition whose every instance shows bodies of its own was not
    /// written, nor were its bodies.
    InstancePrototypeBodiesOmitted,
    /// PMI annotations were not written.
    PmiAnnotationNotWritten,
    /// An all-around scope has fewer than two writable geometric components.
    PmiAllAroundComponentsUnresolved,
    /// Source-object associations were not represented.
    SourceAssociationOmitted,
    /// Display colors had no emitted STEP item.
    DisplayColorUnstyled,
    /// A wire body's direct color alpha was omitted from RGB curve styling.
    WireBodyTransparencyOmitted,
    /// Appearance assets were reduced to `STYLED_ITEM` base colors.
    AppearanceReducedToBaseColor,
    /// Appearance bindings carry source object or channel metadata.
    AppearanceBindingMetadataReduced,
    /// Source attribute records were not written.
    SourceAttributeNotWritten,
    /// Procedural definitions were reduced to solved STEP carriers.
    ProceduralReducedToCarrier,
    /// Helix definitions without a solved carrier were written as fitted
    /// cubic B-splines.
    HelixFittedToBspline,
    /// Procedural definitions without another STEP carrier were written as
    /// sampled cubic B-splines.
    ProceduralSampledToBspline,
    /// A helix definition without a solved carrier had no B-spline within the
    /// linear tolerance, so its carrier was not written.
    HelixFitFailed,
    /// A procedural definition has no emitted native construction or solved carrier.
    ProceduralDefinitionNotWritten,
    /// A region has no writable outer shell.
    RegionNoWritableOuterShell,
    /// A region omitted a void shell with no writable faces.
    RegionOmittedVoidShell,
    /// A solid region was written as a surface model because its shells are open.
    RegionSolidReducedToSheet,
    /// Vertices of unwritable topology were retained as standalone points.
    RegionVerticesRecovered,
    /// Edges of unwritable faces were retained as wire geometry.
    RegionEdgesRecovered,
    /// No geometry was placed in the output representations, while other
    /// records of the document were written. A file that holds no record of
    /// the document charges `export:output.empty` instead.
    NoGeometryWritten,
    /// Different coedge pcurves were compared at discrete parameters.
    EdgePcurveAgreementSampled,
    /// A recovered edge was fitted at sampled parameters.
    EdgePcurveSampledToBspline,
    /// A shell omitted an outer face with no writable topology.
    ShellOmittedOuterFace,
    /// A shell omitted a face whose boundary roles are unspecified.
    ShellOmittedUnclassifiedFace,
    /// A face omitted an outer loop with no writable topology.
    FaceOmittedOuterLoop,
    /// A face omitted an inner loop with no writable topology.
    FaceOmittedInnerLoop,
    /// A face omitted a loop whose boundary role is unspecified.
    FaceOmittedUnclassifiedLoop,
    /// A face has no writable bounds.
    FaceNoWritableBounds,
    /// A face was omitted because one of its bounds could not be written.
    FaceIncompleteBounds,
    /// A face's implicit outer boundary has no STEP bound.
    FaceImplicitOuterBoundUnwritable,
    /// A loop was omitted because its record is missing.
    LoopRecordMissing,
    /// A loop was omitted because its vertex is missing.
    LoopVertexMissing,
    /// A loop omitted a coedge because its record is missing.
    LoopCoedgeRecordMissing,
    /// A loop omitted an edge because the edge is not writable.
    LoopEdgeNotWritable,
    /// A loop has no writable edges.
    LoopNoWritableEdges,
    /// A loop cannot be ordered because a coedge occurs more than once.
    LoopDuplicateCoedge,
    /// A loop cannot be ordered because a coedge is missing.
    LoopCoedgeMissingForOrder,
    /// A loop cannot be ordered because an edge is missing.
    LoopEdgeMissingForOrder,
    /// An edge was omitted because it has no 3D curve.
    EdgeNo3dCurve,
    /// An edge was omitted because its curve is not writable.
    EdgeCurveNotWritable,
    /// An edge was omitted because its curve is unsupported.
    EdgeCurveUnsupported,
    /// A loop has no continuous vertex-to-vertex coedge ordering.
    LoopNoContinuousOrdering,
    /// An edge was omitted because its record is missing.
    EdgeRecordMissing,
    /// An edge was omitted because its start vertex is missing.
    EdgeStartVertexMissing,
    /// An edge was omitted because its end vertex is missing.
    EdgeEndVertexMissing,
    /// An edge was omitted because it has no vertex.
    EdgeNoVertex,
    /// An anchor or REFERENCE expansion passed its value-node limit; the
    /// binding or instance remains source-only.
    ParseExpansionBounded,
    /// Product occurrence expansion stopped at its occurrence limit.
    OccurrenceExpansionOutputTruncated,
    /// Product occurrence expansion stopped a branch at its path-depth limit.
    OccurrenceExpansionDepthTruncated,
    /// A non-manifold edge of a closed shell was written as one edge per
    /// forward and reversed use pair.
    NonManifoldEdgeSplit,
    /// An open edge whose ends coincide within the linear tolerance was
    /// written as one edge: no point of its curve between the ends was found.
    OpenEdgeUnsplit,
    /// A face the source holds with no boundary on a plane, cylinder, or
    /// cone, whose domain is unbounded, or whose stored boundary does not
    /// resolve to a closed loop, is stated partial with no loop.
    TopologyFaceBoundaryUnstated,
    /// An edge vertex lies off its own carrier by more than the document
    /// tolerance, and the edge's parameter range was derived anyway.
    ///
    /// The conic inversion takes the parameter from the IN-PLANE components of
    /// the vertex alone, so the axial and radial offsets cannot change the
    /// answer: the parameter is exact for any vertex off the axis. Refusing it
    /// on the total 3D distance rejects a correct parameter for a defect no
    /// parameter can repair, and costs the whole body downstream. The offset is
    /// reported here instead, with its measured value, because the cost is
    /// continuous and a threshold cannot express it.
    EdgeVertexOffCarrier,
    /// A plane-angle unit named degree states a radian factor other than
    /// `π/180`; the reader reads it as `π/180`.
    AngleUnitFactorCorrected,
    /// The end-to-end lengths of a context's plant items agree on a length
    /// unit other than the one the context declares.
    LengthUnitContradicted,
    /// A parent representation's `MAPPED_ITEM` placed an assembly occurrence
    /// that no STEP relationship associates with that item.
    OccurrencePlacementInferred,
    /// A CSG item's attributes do not resolve; it has no feature and no body.
    CsgItemUnresolved,
    /// A plant CSG shape lists several plant item orientations, so its items
    /// keep their own placements.
    CsgPlantOrientationAmbiguous,
    /// A CSG item has a design feature and no derived body.
    CsgSolidUnsynthesized,
    /// A CSG Boolean tree has design features and no derived body.
    CsgBooleanUnevaluated,
    /// A swept-area profile closed a gap between boundary segment ends.
    CsgProfileGapClosed,
    /// CSG items belong to no product definition, so no design scope files them.
    CsgDesignUnscoped,
    /// The faces on a spindle `TOROIDAL_SURFACE` do not select one sheet, so
    /// the surface carries no sheet selection.
    TorusSheetUnresolved,
}

impl StepLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [StepLossCode] = &[
        Self::ParseNoncanonicalSyntax,
        Self::ParseRecordOmitted,
        Self::ParsePreambleOmitted,
        Self::ParseDraftGrammar,
        Self::ParseEnvelopeIncomplete,
        Self::ContainerMemberNoncanonical,
        Self::DecodeWarning,
        Self::ByteAccountingUnclassified,
        Self::OpaqueRecordPreserved,
        Self::MetadataStringInvalid,
        Self::SchemaObjectIdentifierOutOfRange,
        Self::ImplementationLevelUnverified,
        Self::HeaderMetadataNoncanonical,
        Self::SourceDialectUnverified,
        Self::SourceSchemaEditionAssumed,
        Self::AttributeStringInvalid,
        Self::PlacementReferenceInferred,
        Self::ConflictingRepresentationUnits,
        Self::UncertaintyLengthAmbiguous,
        Self::UncertaintyLengthUnresolved,
        Self::DocumentLengthUnitUnresolved,
        Self::DocumentAngleUnitUnresolved,
        Self::LineParameterScaleUnresolved,
        Self::TopologyRootRejected,
        Self::TopologyRootIncomplete,
        Self::OrientedShellOmitsCfsFaces,
        Self::SeamEdgePcurveUnresolved,
        Self::EdgeNoSurfaceOrCurveForPcurve,
        Self::PcurveEndpointsDiscontinuous,
        Self::PcurveLocusDiscontinuous,
        Self::PcurveGlobalFidelityUnproved,
        Self::PcurveAssociationAmbiguous,
        Self::PcurveCandidatesCarrierUnresolved,
        Self::FaceMultipleOuterBounds,
        Self::ShellDisconnectedFaces,
        Self::NauoPlacementUnresolved,
        Self::NauoPlacementAmbiguous,
        Self::BodyConflictingMappedPlacements,
        Self::PresentationAnnotationTextUnordered,
        Self::PresentationAnnotationPlacementAmbiguous,
        Self::DimensionalNominalAmbiguous,
        Self::DimensionalUnnamedMeasureAmbiguous,
        Self::PmiLengthUnitUnresolved,
        Self::PmiAngleUnitUnresolved,
        Self::PmiDatumSystemInvalid,
        Self::PmiZoneUnresolved,
        Self::PmiDimensionModifierUnresolved,
        Self::PmiDimensionKindQuantityMismatch,
        Self::DimensionalLimitsOmitted,
        Self::ConflictingScalarColors,
        Self::SurfaceSideInvalid,
        Self::SurfaceTransparencyConflict,
        Self::SurfaceTransparencyMisplaced,
        Self::ContextDependentStyleUnresolved,
        Self::DrawingRecordTooFewParameters,
        Self::DrawingOrderUnstatable,
        Self::DrawingRelationshipUntypedTarget,
        Self::DrawingRelationshipTargetAmbiguous,
        Self::DrawingSheetRevisionUnresolved,
        Self::DrawingRevisionSheetUnresolved,
        Self::DraughtingSemanticDefinitionUntyped,
        Self::DraughtingAssociatedItemUntyped,
        Self::TessellationItemBodyUnresolved,
        Self::TessellationInvalidPayload,
        Self::TessellationItemUndeclared,
        Self::TessellationPlacementUnresolved,
        Self::TessellationPlacementAmbiguous,
        Self::ValidationMeasureUnitUnresolved,
        Self::ValidationItemUnsupported,
        Self::NoExportableSolids,
        Self::LayerItemWithoutCarrier,
        Self::LayerHierarchyFlattened,
        Self::OccurrenceUnresolvedParent,
        Self::OccurrenceNoLocalProduct,
        Self::OccurrencePlacementNotRigid,
        Self::BodyNonRigidTransform,
        Self::HiddenBodyVisibilityUnsupported,
        Self::HiddenAppearanceVisibilityUnsupported,
        Self::HiddenPresentationLayerVisibilityUnsupported,
        Self::HiddenPmiVisibilityUnsupported,
        Self::WireShellFreeVertices,
        Self::BodyWithoutProductRepresentation,
        Self::SurfaceShellWireTopologyOmitted,
        Self::TessellationRequiresAp242,
        Self::TessellationFeatureEdges,
        Self::TessellationCornerNormals,
        Self::TessellationTriangleGroups,
        Self::TessellationTextureAssignments,
        Self::TessellationInvalidCardinality,
        Self::TessellationBodyLinkUnwritable,
        Self::TessellationMetadataReduced,
        Self::TopologyUnreachableFromRegion,
        Self::AnalyticSurfaceNormalized,
        Self::EllipticalConeReduced,
        Self::CurvelessEdgeOmitted,
        Self::UnknownSurfaceFaceOmitted,
        Self::GeometryCarrierNotWritten,
        Self::PcurveCarrierUnwritable,
        Self::PcurveOnFittedHelixSurface,
        Self::AssemblyOccurrenceOmittedNoParentProduct,
        Self::RegionNoShellList,
        Self::WireRegionNoConnectedEdgeSet,
        Self::WireRegionMissingShell,
        Self::HiddenBodyOmitted,
        Self::HiddenPresentationLayerOmitted,
        Self::AppearanceBindingMissingAsset,
        Self::AppearanceBindingNoBaseColor,
        Self::AppearanceBindingTargetConflict,
        Self::CoedgePcurveNoGeometry,
        Self::CoedgePcurveNativeMetadata,
        Self::PcurveUseNativeMetadata,
        Self::CoedgeUseCurveNotRepresented,
        Self::TopologyMetadataNotRepresented,
        Self::SubdOmitted,
        Self::ParametricDesignRecordsOmitted,
        Self::DrawingPresentationRecordsOmitted,
        Self::SemanticAnnotationOmitted,
        Self::DocumentAssetOmitted,
        Self::AssemblyJointOmitted,
        Self::ProductNonPartKind,
        Self::ProductBomPropertyOmitted,
        Self::OccurrenceExternalProduct,
        Self::OccurrenceNoWritableProduct,
        Self::OccurrenceMetadataOmitted,
        Self::InstanceDefinitionSplit,
        Self::InstancePathUnresolved,
        Self::InstancePrototypeBodiesOmitted,
        Self::PmiAnnotationNotWritten,
        Self::PmiAllAroundComponentsUnresolved,
        Self::SourceAssociationOmitted,
        Self::DisplayColorUnstyled,
        Self::WireBodyTransparencyOmitted,
        Self::AppearanceReducedToBaseColor,
        Self::AppearanceBindingMetadataReduced,
        Self::SourceAttributeNotWritten,
        Self::ProceduralReducedToCarrier,
        Self::HelixFittedToBspline,
        Self::ProceduralSampledToBspline,
        Self::HelixFitFailed,
        Self::ProceduralDefinitionNotWritten,
        Self::RegionNoWritableOuterShell,
        Self::RegionOmittedVoidShell,
        Self::RegionSolidReducedToSheet,
        Self::RegionVerticesRecovered,
        Self::RegionEdgesRecovered,
        Self::NoGeometryWritten,
        Self::EdgePcurveAgreementSampled,
        Self::EdgePcurveSampledToBspline,
        Self::ShellOmittedOuterFace,
        Self::ShellOmittedUnclassifiedFace,
        Self::FaceOmittedOuterLoop,
        Self::FaceOmittedInnerLoop,
        Self::FaceOmittedUnclassifiedLoop,
        Self::FaceNoWritableBounds,
        Self::FaceIncompleteBounds,
        Self::FaceImplicitOuterBoundUnwritable,
        Self::LoopRecordMissing,
        Self::LoopVertexMissing,
        Self::LoopCoedgeRecordMissing,
        Self::LoopEdgeNotWritable,
        Self::LoopNoWritableEdges,
        Self::LoopDuplicateCoedge,
        Self::LoopCoedgeMissingForOrder,
        Self::LoopEdgeMissingForOrder,
        Self::EdgeNo3dCurve,
        Self::EdgeCurveNotWritable,
        Self::EdgeCurveUnsupported,
        Self::LoopNoContinuousOrdering,
        Self::EdgeRecordMissing,
        Self::EdgeStartVertexMissing,
        Self::EdgeEndVertexMissing,
        Self::EdgeNoVertex,
        Self::ParseExpansionBounded,
        Self::OccurrenceExpansionOutputTruncated,
        Self::OccurrenceExpansionDepthTruncated,
        Self::NonManifoldEdgeSplit,
        Self::OpenEdgeUnsplit,
        Self::TopologyFaceBoundaryUnstated,
        Self::EdgeVertexOffCarrier,
        Self::AngleUnitFactorCorrected,
        Self::LengthUnitContradicted,
        Self::OccurrencePlacementInferred,
        Self::CsgItemUnresolved,
        Self::CsgPlantOrientationAmbiguous,
        Self::CsgSolidUnsynthesized,
        Self::CsgBooleanUnevaluated,
        Self::CsgProfileGapClosed,
        Self::CsgDesignUnscoped,
        Self::TorusSheetUnresolved,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::ContainerMemberNoncanonical => "container.member-noncanonical",
            Self::ParseNoncanonicalSyntax => "parse.noncanonical-syntax",
            Self::ParseRecordOmitted => "parse.record-omitted",
            Self::ParsePreambleOmitted => "parse.preamble-omitted",
            Self::ParseDraftGrammar => "parse.draft-grammar",
            Self::ParseEnvelopeIncomplete => "parse.envelope-incomplete",
            Self::DecodeWarning => "decode.warning",
            Self::ByteAccountingUnclassified => "decode.byte-accounting-unclassified",
            Self::OpaqueRecordPreserved => "decode.opaque-record-preserved",
            Self::MetadataStringInvalid => "metadata.string-invalid",
            Self::SchemaObjectIdentifierOutOfRange => {
                "metadata.schema-object-identifier-out-of-range"
            }
            Self::ImplementationLevelUnverified => "parse.implementation-level-unverified",
            Self::HeaderMetadataNoncanonical => "parse.header-metadata-noncanonical",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::SourceSchemaEditionAssumed => "source.schema-edition-assumed",
            Self::AttributeStringInvalid => "attribute.string-invalid",
            Self::PlacementReferenceInferred => "geometry.placement-reference-inferred",
            Self::ConflictingRepresentationUnits => "geometry.conflicting-representation-units",
            Self::UncertaintyLengthAmbiguous => "geometry.uncertainty-length-ambiguous",
            Self::UncertaintyLengthUnresolved => "geometry.uncertainty-length-unresolved",
            Self::DocumentLengthUnitUnresolved => "geometry.document-length-unit-unresolved",
            Self::DocumentAngleUnitUnresolved => "geometry.document-angle-unit-unresolved",
            Self::LineParameterScaleUnresolved => "geometry.line-parameter-scale-unresolved",
            Self::TopologyRootRejected => "topology.root-rejected",
            Self::TopologyRootIncomplete => "topology.root-incomplete",
            Self::OrientedShellOmitsCfsFaces => "topology.oriented-shell-omits-cfs-faces",
            Self::SeamEdgePcurveUnresolved => "topology.seam-edge-pcurve-unresolved",
            Self::EdgeNoSurfaceOrCurveForPcurve => "topology.edge-no-surface-or-curve-for-pcurve",
            Self::PcurveEndpointsDiscontinuous => "topology.pcurve-endpoints-discontinuous",
            Self::PcurveLocusDiscontinuous => "topology.pcurve-locus-discontinuous",
            Self::PcurveGlobalFidelityUnproved => "topology.pcurve-global-fidelity-unproved",
            Self::PcurveAssociationAmbiguous => "topology.pcurve-association-ambiguous",
            Self::PcurveCandidatesCarrierUnresolved => {
                "topology.pcurve-candidates-carrier-unresolved"
            }
            Self::FaceMultipleOuterBounds => "topology.face-multiple-outer-bounds",
            Self::ShellDisconnectedFaces => "topology.shell-disconnected-faces",
            Self::NauoPlacementUnresolved => "product.nauo-placement-unresolved",
            Self::NauoPlacementAmbiguous => "product.nauo-placement-ambiguous",
            Self::BodyConflictingMappedPlacements => "product.body-conflicting-mapped-placements",
            Self::PresentationAnnotationTextUnordered => {
                "pmi.presentation-annotation-text-unordered"
            }
            Self::PresentationAnnotationPlacementAmbiguous => {
                "pmi.presentation-annotation-placement-ambiguous"
            }
            Self::DimensionalNominalAmbiguous => "pmi.dimensional-nominal-ambiguous",
            Self::DimensionalUnnamedMeasureAmbiguous => "pmi.dimensional-unnamed-measure-ambiguous",
            Self::PmiLengthUnitUnresolved => "pmi.length-unit-unresolved",
            Self::PmiAngleUnitUnresolved => "pmi.angle-unit-unresolved",
            Self::PmiDatumSystemInvalid => "pmi.datum-system-invalid",
            Self::PmiZoneUnresolved => "pmi.zone-unresolved",
            Self::PmiDimensionModifierUnresolved => "pmi.dimension-modifier-unresolved",
            Self::PmiDimensionKindQuantityMismatch => "pmi.dimension-kind-quantity-mismatch",
            Self::DimensionalLimitsOmitted => "pmi.dimensional-limits-omitted",
            Self::ConflictingScalarColors => "presentation.conflicting-scalar-colors",
            Self::SurfaceSideInvalid => "presentation.surface-side-invalid",
            Self::SurfaceTransparencyConflict => "presentation.surface-transparency-conflict",
            Self::SurfaceTransparencyMisplaced => "presentation.surface-transparency-misplaced",
            Self::ContextDependentStyleUnresolved => {
                "presentation.context-dependent-style-unresolved"
            }
            Self::DrawingRecordTooFewParameters => "drawing.record-too-few-parameters",
            Self::DrawingOrderUnstatable => "drawing.order-unstatable",
            Self::DrawingRelationshipUntypedTarget => "drawing.relationship-untyped-target",
            Self::DrawingRelationshipTargetAmbiguous => "drawing.relationship-target-ambiguous",
            Self::DrawingSheetRevisionUnresolved => "drawing.sheet-revision-unresolved",
            Self::DrawingRevisionSheetUnresolved => "drawing.revision-sheet-unresolved",
            Self::DraughtingSemanticDefinitionUntyped => {
                "drawing.draughting-semantic-definition-untyped"
            }
            Self::DraughtingAssociatedItemUntyped => "drawing.draughting-associated-item-untyped",
            Self::TessellationInvalidPayload => "tessellation.invalid-payload",
            Self::TessellationItemBodyUnresolved => "tessellation.item-body-unresolved",
            Self::TessellationItemUndeclared => "tessellation.item-undeclared",
            Self::TessellationPlacementUnresolved => "tessellation.placement-unresolved",
            Self::TessellationPlacementAmbiguous => "tessellation.placement-ambiguous",
            Self::ValidationMeasureUnitUnresolved => "validation.measure-unit-unresolved",
            Self::ValidationItemUnsupported => "validation.item-unsupported",
            Self::NoExportableSolids => "export.no-exportable-solids",
            Self::LayerItemWithoutCarrier => "layer.item-without-carrier",
            Self::LayerHierarchyFlattened => "layer.hierarchy-flattened",
            Self::OccurrenceUnresolvedParent => "occurrence.unresolved-parent",
            Self::OccurrenceNoLocalProduct => "occurrence.no-local-product",
            Self::OccurrencePlacementNotRigid => "occurrence.placement-not-rigid",
            Self::BodyNonRigidTransform => "body.non-rigid-transform",
            Self::HiddenBodyVisibilityUnsupported => "body.hidden-visibility-unsupported",
            Self::HiddenAppearanceVisibilityUnsupported => {
                "presentation.hidden-appearance-visibility-unsupported"
            }
            Self::HiddenPresentationLayerVisibilityUnsupported => {
                "presentation.hidden-layer-visibility-unsupported"
            }
            Self::HiddenPmiVisibilityUnsupported => "pmi.hidden-visibility-unsupported",
            Self::BodyWithoutProductRepresentation => "product.body-without-representation",
            Self::SurfaceShellWireTopologyOmitted => "topology.surface-shell-wire-omitted",
            Self::WireShellFreeVertices => "topology.wire-shell-free-vertices",
            Self::TessellationRequiresAp242 => "tessellation.requires-ap242",
            Self::TessellationFeatureEdges => "tessellation.feature-edges",
            Self::TessellationCornerNormals => "tessellation.corner-normals",
            Self::TessellationTriangleGroups => "tessellation.triangle-groups",
            Self::TessellationTextureAssignments => "tessellation.texture-assignments",
            Self::TessellationInvalidCardinality => "tessellation.invalid-cardinality",
            Self::TessellationBodyLinkUnwritable => "tessellation.body-link-unwritable",
            Self::TessellationMetadataReduced => "tessellation.metadata-reduced",
            Self::TopologyUnreachableFromRegion => "topology.unreachable-from-region",
            Self::AnalyticSurfaceNormalized => "geometry.analytic-surface-normalized",
            Self::EllipticalConeReduced => "geometry.elliptical-cone-reduced",
            Self::CurvelessEdgeOmitted => "geometry.curveless-edge-omitted",
            Self::UnknownSurfaceFaceOmitted => "geometry.unknown-surface-face-omitted",
            Self::GeometryCarrierNotWritten => "geometry.carrier-not-written",
            Self::PcurveCarrierUnwritable => "pcurve.carrier-unwritable",
            Self::PcurveOnFittedHelixSurface => "pcurve.on-fitted-helix-surface",
            Self::AssemblyOccurrenceOmittedNoParentProduct => {
                "assembly.occurrence-omitted-no-parent-product"
            }
            Self::RegionNoShellList => "topology.region-no-shell-list",
            Self::WireRegionNoConnectedEdgeSet => "topology.wire-region-no-connected-edge-set",
            Self::WireRegionMissingShell => "topology.wire-region-missing-shell",
            Self::HiddenBodyOmitted => "body.hidden-omitted",
            Self::HiddenPresentationLayerOmitted => "presentation.hidden-layer-omitted",
            Self::AppearanceBindingMissingAsset => "appearance.binding-missing-asset",
            Self::AppearanceBindingNoBaseColor => "appearance.binding-no-base-color",
            Self::AppearanceBindingTargetConflict => "appearance.binding-target-conflict",
            Self::CoedgePcurveNoGeometry => "pcurve.coedge-no-geometry",
            Self::CoedgePcurveNativeMetadata => "pcurve.coedge-native-metadata",
            Self::PcurveUseNativeMetadata => "pcurve.use-native-metadata",
            Self::CoedgeUseCurveNotRepresented => "topology.coedge-use-curve-not-represented",
            Self::TopologyMetadataNotRepresented => "topology.metadata-not-represented",
            Self::SubdOmitted => "geometry.subd-omitted",
            Self::ParametricDesignRecordsOmitted => "design.parametric-records-omitted",
            Self::DrawingPresentationRecordsOmitted => "presentation.drawing-records-omitted",
            Self::SemanticAnnotationOmitted => "pmi.semantic-annotation-omitted",
            Self::DocumentAssetOmitted => "asset.document-omitted",
            Self::AssemblyJointOmitted => "assembly.joint-omitted",
            Self::ProductNonPartKind => "product.non-part-kind",
            Self::ProductBomPropertyOmitted => "product.bom-property-omitted",
            Self::OccurrenceExternalProduct => "occurrence.external-product",
            Self::OccurrenceNoWritableProduct => "occurrence.no-writable-product",
            Self::OccurrenceMetadataOmitted => "occurrence.metadata-omitted",
            Self::InstanceDefinitionSplit => "product.instance-definition-split",
            Self::InstancePathUnresolved => "product.instance-path-unresolved",
            Self::InstancePrototypeBodiesOmitted => "product.instance-prototype-bodies-omitted",
            Self::PmiAnnotationNotWritten => "pmi.annotation-not-written",
            Self::PmiAllAroundComponentsUnresolved => "pmi.all-around-components-unresolved",
            Self::SourceAssociationOmitted => "source.association-omitted",
            Self::DisplayColorUnstyled => "appearance.display-color-unstyled",
            Self::WireBodyTransparencyOmitted => "appearance.wire-body-transparency-omitted",
            Self::AppearanceReducedToBaseColor => "appearance.reduced-to-base-color",
            Self::AppearanceBindingMetadataReduced => "appearance.binding-metadata-reduced",
            Self::SourceAttributeNotWritten => "attribute.source-record-not-written",
            Self::ProceduralReducedToCarrier => "geometry.procedural-reduced-to-carrier",
            Self::HelixFittedToBspline => "geometry.helix-fitted-to-bspline",
            Self::ProceduralSampledToBspline => "geometry.procedural-sampled-to-bspline",
            Self::HelixFitFailed => "geometry.helix-fit-failed",
            Self::ProceduralDefinitionNotWritten => "geometry.procedural-definition-not-written",
            Self::RegionNoWritableOuterShell => "topology.region-no-writable-outer-shell",
            Self::RegionOmittedVoidShell => "topology.region-omitted-void-shell",
            Self::RegionSolidReducedToSheet => "topology.region-solid-reduced-to-sheet",
            Self::RegionVerticesRecovered => "topology.region-vertices-recovered",
            Self::RegionEdgesRecovered => "topology.region-edges-recovered",
            Self::NoGeometryWritten => "export.no-geometry-written",
            Self::EdgePcurveAgreementSampled => "geometry.edge-pcurve-agreement-sampled",
            Self::EdgePcurveSampledToBspline => "geometry.edge-pcurve-sampled-to-bspline",
            Self::ShellOmittedOuterFace => "topology.shell-omitted-outer-face",
            Self::ShellOmittedUnclassifiedFace => "topology.shell-omitted-unclassified-face",
            Self::FaceOmittedOuterLoop => "topology.face-omitted-outer-loop",
            Self::FaceOmittedInnerLoop => "topology.face-omitted-inner-loop",
            Self::FaceOmittedUnclassifiedLoop => "topology.face-omitted-unclassified-loop",
            Self::FaceNoWritableBounds => "topology.face-no-writable-bounds",
            Self::FaceIncompleteBounds => "topology.face-incomplete-bounds",
            Self::FaceImplicitOuterBoundUnwritable => {
                "topology.face-implicit-outer-bound-unwritable"
            }
            Self::LoopRecordMissing => "topology.loop-record-missing",
            Self::LoopVertexMissing => "topology.loop-vertex-missing",
            Self::LoopCoedgeRecordMissing => "topology.loop-coedge-record-missing",
            Self::LoopEdgeNotWritable => "topology.loop-edge-not-writable",
            Self::LoopNoWritableEdges => "topology.loop-no-writable-edges",
            Self::LoopDuplicateCoedge => "topology.loop-duplicate-coedge",
            Self::LoopCoedgeMissingForOrder => "topology.loop-coedge-missing-for-order",
            Self::LoopEdgeMissingForOrder => "topology.loop-edge-missing-for-order",
            Self::EdgeNo3dCurve => "topology.edge-no-3d-curve",
            Self::EdgeCurveNotWritable => "topology.edge-curve-not-writable",
            Self::EdgeCurveUnsupported => "topology.edge-curve-unsupported",
            Self::LoopNoContinuousOrdering => "topology.loop-no-continuous-ordering",
            Self::EdgeRecordMissing => "topology.edge-record-missing",
            Self::EdgeStartVertexMissing => "topology.edge-start-vertex-missing",
            Self::EdgeEndVertexMissing => "topology.edge-end-vertex-missing",
            Self::EdgeNoVertex => "topology.edge-no-vertex",
            Self::ParseExpansionBounded => "parse.expansion-bounded",
            Self::OccurrenceExpansionOutputTruncated => "occurrence.expansion-output-truncated",
            Self::OccurrenceExpansionDepthTruncated => "occurrence.expansion-depth-truncated",
            Self::NonManifoldEdgeSplit => "topology.non-manifold-edge-split",
            Self::OpenEdgeUnsplit => "topology.open-edge-unsplit",
            Self::TopologyFaceBoundaryUnstated => "topology.face-boundary-unstated",
            Self::EdgeVertexOffCarrier => "geometry.edge-vertex-off-carrier",
            Self::AngleUnitFactorCorrected => "geometry.angle-unit-factor-corrected",
            Self::LengthUnitContradicted => "geometry.length-unit-contradicted",
            Self::OccurrencePlacementInferred => "product.occurrence-placement-inferred",
            Self::CsgItemUnresolved => "csg.item-unresolved",
            Self::CsgPlantOrientationAmbiguous => "csg.plant-orientation-ambiguous",
            Self::CsgSolidUnsynthesized => "csg.solid-unsynthesized",
            Self::CsgBooleanUnevaluated => "csg.boolean-unevaluated",
            Self::CsgProfileGapClosed => "csg.profile-gap-closed",
            Self::CsgDesignUnscoped => "csg.design-unscoped",
            Self::TorusSheetUnresolved => "geometry.torus-sheet-unresolved",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::ByteAccountingUnclassified
            | Self::DocumentLengthUnitUnresolved
            | Self::DocumentAngleUnitUnresolved
            | Self::LineParameterScaleUnresolved
            | Self::TopologyRootRejected
            | Self::TopologyRootIncomplete
            | Self::PcurveEndpointsDiscontinuous
            | Self::PcurveLocusDiscontinuous
            | Self::NauoPlacementUnresolved
            | Self::NauoPlacementAmbiguous
            | Self::BodyConflictingMappedPlacements
            | Self::PmiLengthUnitUnresolved
            | Self::PmiAngleUnitUnresolved
            | Self::ValidationMeasureUnitUnresolved
            | Self::RegionNoShellList
            | Self::RegionNoWritableOuterShell
            | Self::RegionOmittedVoidShell
            | Self::RegionSolidReducedToSheet
            | Self::RegionVerticesRecovered
            | Self::RegionEdgesRecovered
            | Self::NoGeometryWritten
            | Self::ShellOmittedOuterFace
            | Self::ShellOmittedUnclassifiedFace
            | Self::FaceOmittedOuterLoop
            | Self::FaceOmittedUnclassifiedLoop
            | Self::FaceNoWritableBounds
            | Self::FaceIncompleteBounds
            | Self::FaceImplicitOuterBoundUnwritable
            | Self::LoopCoedgeRecordMissing
            | Self::LoopEdgeNotWritable
            | Self::LoopDuplicateCoedge
            | Self::LoopCoedgeMissingForOrder
            | Self::LoopEdgeMissingForOrder
            | Self::LoopNoContinuousOrdering => Severity::Error,
            Self::TessellationMetadataReduced
            | Self::CoedgePcurveNativeMetadata
            | Self::PcurveUseNativeMetadata
            | Self::CoedgeUseCurveNotRepresented
            | Self::TopologyMetadataNotRepresented
            | Self::ParametricDesignRecordsOmitted
            | Self::DrawingPresentationRecordsOmitted
            | Self::DocumentAssetOmitted
            | Self::AssemblyJointOmitted
            | Self::ProductNonPartKind
            | Self::ProductBomPropertyOmitted
            | Self::OccurrenceMetadataOmitted
            | Self::InstanceDefinitionSplit
            | Self::SourceAssociationOmitted
            | Self::DisplayColorUnstyled
            | Self::WireBodyTransparencyOmitted
            | Self::AppearanceReducedToBaseColor
            | Self::AppearanceBindingMetadataReduced
            | Self::SourceAttributeNotWritten
            | Self::ProceduralReducedToCarrier
            | Self::HelixFittedToBspline
            | Self::ProceduralSampledToBspline
            | Self::NonManifoldEdgeSplit
            | Self::OccurrencePlacementInferred => Severity::Info,
            _ => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ContainerMemberNoncanonical
            | Self::ParseNoncanonicalSyntax
            | Self::ParsePreambleOmitted
            | Self::ParseEnvelopeIncomplete
            | Self::ParseRecordOmitted
            | Self::OrientedShellOmitsCfsFaces
            | Self::HeaderMetadataNoncanonical => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::DecodeWarning
            | Self::ByteAccountingUnclassified
            | Self::PcurveGlobalFidelityUnproved
            | Self::OccurrenceExpansionOutputTruncated
            | Self::OccurrenceExpansionDepthTruncated => LossTaxonomy::DecodeDiagnostic,
            Self::ParseDraftGrammar => LossTaxonomy::SourceDialectUnverified,
            Self::OpaqueRecordPreserved
            | Self::DrawingRecordTooFewParameters
            | Self::DrawingOrderUnstatable
            | Self::ParseExpansionBounded => LossTaxonomy::RecordNotTyped,
            Self::ImplementationLevelUnverified
            | Self::SourceDialectUnverified
            | Self::SourceSchemaEditionAssumed => LossTaxonomy::SourceDialectUnverified,
            Self::MetadataStringInvalid
            | Self::SchemaObjectIdentifierOutOfRange
            | Self::PresentationAnnotationTextUnordered
            | Self::PresentationAnnotationPlacementAmbiguous
            | Self::DimensionalNominalAmbiguous
            | Self::DimensionalUnnamedMeasureAmbiguous
            | Self::ConflictingScalarColors
            | Self::SurfaceSideInvalid
            | Self::SurfaceTransparencyConflict
            | Self::SurfaceTransparencyMisplaced
            | Self::ContextDependentStyleUnresolved
            | Self::DrawingRelationshipUntypedTarget
            | Self::DraughtingSemanticDefinitionUntyped
            | Self::DraughtingAssociatedItemUntyped
            | Self::ProductNonPartKind
            | Self::ProductBomPropertyOmitted
            | Self::OccurrenceNoWritableProduct
            | Self::OccurrenceMetadataOmitted
            | Self::InstanceDefinitionSplit
            | Self::ValidationItemUnsupported => LossTaxonomy::MetadataNotTransferred,
            Self::AttributeStringInvalid
            | Self::LayerItemWithoutCarrier
            | Self::LayerHierarchyFlattened
            | Self::HiddenBodyVisibilityUnsupported
            | Self::HiddenAppearanceVisibilityUnsupported
            | Self::HiddenPresentationLayerVisibilityUnsupported
            | Self::HiddenPmiVisibilityUnsupported
            | Self::HiddenPresentationLayerOmitted
            | Self::TessellationFeatureEdges
            | Self::TessellationCornerNormals
            | Self::TessellationTriangleGroups
            | Self::TessellationTextureAssignments
            | Self::TessellationMetadataReduced
            | Self::CoedgeUseCurveNotRepresented
            | Self::TopologyMetadataNotRepresented
            | Self::DrawingPresentationRecordsOmitted
            | Self::DisplayColorUnstyled
            | Self::SourceAttributeNotWritten => LossTaxonomy::AttributesNotTransferred,
            Self::PlacementReferenceInferred => LossTaxonomy::CarrierAxisInferred,
            Self::ConflictingRepresentationUnits
            | Self::UncertaintyLengthAmbiguous
            | Self::UncertaintyLengthUnresolved
            | Self::DocumentLengthUnitUnresolved
            | Self::DocumentAngleUnitUnresolved
            | Self::LineParameterScaleUnresolved
            | Self::PmiLengthUnitUnresolved
            | Self::PmiAngleUnitUnresolved
            | Self::ValidationMeasureUnitUnresolved
            | Self::GeometryCarrierNotWritten
            | Self::HelixFitFailed
            | Self::InstancePrototypeBodiesOmitted
            | Self::ProceduralDefinitionNotWritten => LossTaxonomy::GeometryNotTransferred,
            Self::TopologyRootRejected
            | Self::TopologyRootIncomplete
            | Self::OccurrenceUnresolvedParent
            | Self::OccurrenceNoLocalProduct
            | Self::WireShellFreeVertices
            | Self::BodyWithoutProductRepresentation
            | Self::SurfaceShellWireTopologyOmitted
            | Self::TessellationBodyLinkUnwritable
            | Self::TopologyUnreachableFromRegion
            | Self::RegionNoShellList
            | Self::WireRegionNoConnectedEdgeSet
            | Self::WireRegionMissingShell
            | Self::RegionNoWritableOuterShell
            | Self::RegionOmittedVoidShell
            | Self::RegionSolidReducedToSheet
            | Self::RegionVerticesRecovered
            | Self::RegionEdgesRecovered
            | Self::ShellOmittedOuterFace
            | Self::ShellOmittedUnclassifiedFace
            | Self::FaceOmittedOuterLoop
            | Self::FaceOmittedUnclassifiedLoop
            | Self::FaceOmittedInnerLoop
            | Self::FaceNoWritableBounds
            | Self::FaceIncompleteBounds
            | Self::FaceImplicitOuterBoundUnwritable
            | Self::LoopRecordMissing
            | Self::LoopVertexMissing
            | Self::LoopCoedgeRecordMissing
            | Self::LoopEdgeNotWritable
            | Self::LoopNoWritableEdges
            | Self::LoopDuplicateCoedge
            | Self::LoopCoedgeMissingForOrder
            | Self::LoopEdgeMissingForOrder
            | Self::EdgeNo3dCurve
            | Self::EdgeCurveNotWritable
            | Self::EdgeCurveUnsupported
            | Self::LoopNoContinuousOrdering
            | Self::EdgeRecordMissing
            | Self::EdgeStartVertexMissing
            | Self::EdgeEndVertexMissing
            | Self::EdgeNoVertex
            | Self::NonManifoldEdgeSplit
            | Self::OpenEdgeUnsplit
            | Self::TopologyFaceBoundaryUnstated => LossTaxonomy::TopologyNotTransferred,
            Self::SeamEdgePcurveUnresolved
            | Self::EdgeNoSurfaceOrCurveForPcurve
            | Self::PcurveAssociationAmbiguous
            | Self::PcurveCandidatesCarrierUnresolved
            | Self::DrawingRelationshipTargetAmbiguous
            | Self::DrawingSheetRevisionUnresolved
            | Self::DrawingRevisionSheetUnresolved
            | Self::TessellationItemBodyUnresolved
            | Self::TessellationItemUndeclared
            | Self::TessellationPlacementUnresolved
            | Self::TessellationPlacementAmbiguous => LossTaxonomy::ReferenceGraphNotClosed,
            Self::PcurveEndpointsDiscontinuous
            | Self::PcurveLocusDiscontinuous
            | Self::PcurveCarrierUnwritable
            | Self::PcurveOnFittedHelixSurface
            | Self::CoedgePcurveNoGeometry
            | Self::CoedgePcurveNativeMetadata
            | Self::PcurveUseNativeMetadata => LossTaxonomy::PcurveOmitted,
            Self::FaceMultipleOuterBounds
            | Self::ShellDisconnectedFaces
            | Self::EdgeVertexOffCarrier => LossTaxonomy::SourceTopologyInvalid,
            Self::CsgPlantOrientationAmbiguous => LossTaxonomy::BodyTransformNotApplied,
            Self::CsgItemUnresolved | Self::CsgSolidUnsynthesized | Self::CsgBooleanUnevaluated => {
                LossTaxonomy::GeometryNotTransferred
            }
            Self::CsgProfileGapClosed
            | Self::AngleUnitFactorCorrected
            | Self::LengthUnitContradicted
            | Self::OccurrencePlacementInferred
            | Self::TorusSheetUnresolved => LossTaxonomy::DecodeDiagnostic,
            Self::CsgDesignUnscoped => LossTaxonomy::MetadataNotTransferred,
            Self::NauoPlacementUnresolved
            | Self::NauoPlacementAmbiguous
            | Self::BodyConflictingMappedPlacements
            | Self::AssemblyOccurrenceOmittedNoParentProduct
            | Self::InstancePathUnresolved
            | Self::AssemblyJointOmitted => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::NoExportableSolids => LossTaxonomy::NoExportableSolids,
            Self::NoGeometryWritten => LossTaxonomy::GeometryNotTransferred,
            Self::EdgePcurveAgreementSampled | Self::EdgePcurveSampledToBspline => {
                LossTaxonomy::GeometryAgreementSampled
            }
            Self::OccurrencePlacementNotRigid | Self::BodyNonRigidTransform => {
                LossTaxonomy::BodyTransformNotApplied
            }
            Self::TessellationRequiresAp242
            | Self::TessellationInvalidCardinality
            | Self::TessellationInvalidPayload => LossTaxonomy::TessellationOmitted,
            Self::AnalyticSurfaceNormalized => LossTaxonomy::AnalyticSurfaceNormalized,
            Self::EllipticalConeReduced => LossTaxonomy::EllipticalConeReduced,
            Self::CurvelessEdgeOmitted => LossTaxonomy::CurvelessEdgeOmitted,
            Self::UnknownSurfaceFaceOmitted => LossTaxonomy::UnknownSurfaceFaceOmitted,
            Self::HiddenBodyOmitted => LossTaxonomy::HiddenBodyOmitted,
            Self::AppearanceBindingMissingAsset
            | Self::AppearanceBindingNoBaseColor
            | Self::AppearanceBindingTargetConflict
            | Self::WireBodyTransparencyOmitted => LossTaxonomy::MaterialNotTransferred,
            Self::SubdOmitted => LossTaxonomy::SubdOmitted,
            Self::ParametricDesignRecordsOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::SemanticAnnotationOmitted
            | Self::PmiAnnotationNotWritten
            | Self::PmiAllAroundComponentsUnresolved
            | Self::PmiDatumSystemInvalid
            | Self::PmiZoneUnresolved
            | Self::PmiDimensionModifierUnresolved
            | Self::PmiDimensionKindQuantityMismatch
            | Self::DimensionalLimitsOmitted => LossTaxonomy::PmiOmitted,
            Self::DocumentAssetOmitted => LossTaxonomy::AssetNotTransferred,
            Self::OccurrenceExternalProduct => LossTaxonomy::AssemblyComponentsExternal,
            Self::SourceAssociationOmitted => LossTaxonomy::SourceAssociationOmitted,
            Self::AppearanceReducedToBaseColor | Self::AppearanceBindingMetadataReduced => {
                LossTaxonomy::AppearanceReduced
            }
            Self::ProceduralReducedToCarrier
            | Self::HelixFittedToBspline
            | Self::ProceduralSampledToBspline => LossTaxonomy::ProceduralReduced,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        cadmpeg_ir::report::loss::NamespacedLossKind::new(
            NAMESPACE,
            self.code(),
            self.shared_taxonomy(),
        )
        .with_strict_floor(self.strict_floor())
        .into()
    }

    /// The strict floor `step.md` (Part 21 mapping) states: a repaired
    /// noncanonical record pins Warning.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::ParseNoncanonicalSyntax => Some(Severity::Warning),
            _ => None,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `step/<local>`. Severity and strict floor come
    /// from the local code.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::StepLossCode;
    use cadmpeg_ir::report::{strictness::Strictness, Severity};
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = StepLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "parse.noncanonical-syntax",
                "parse.record-omitted",
                "parse.preamble-omitted",
                "parse.draft-grammar",
                "parse.envelope-incomplete",
                "container.member-noncanonical",
                "decode.warning",
                "decode.byte-accounting-unclassified",
                "decode.opaque-record-preserved",
                "metadata.string-invalid",
                "metadata.schema-object-identifier-out-of-range",
                "parse.implementation-level-unverified",
                "parse.header-metadata-noncanonical",
                "source.dialect-unverified",
                "source.schema-edition-assumed",
                "attribute.string-invalid",
                "geometry.placement-reference-inferred",
                "geometry.conflicting-representation-units",
                "geometry.uncertainty-length-ambiguous",
                "geometry.uncertainty-length-unresolved",
                "geometry.document-length-unit-unresolved",
                "geometry.document-angle-unit-unresolved",
                "geometry.line-parameter-scale-unresolved",
                "topology.root-rejected",
                "topology.root-incomplete",
                "topology.oriented-shell-omits-cfs-faces",
                "topology.seam-edge-pcurve-unresolved",
                "topology.edge-no-surface-or-curve-for-pcurve",
                "topology.pcurve-endpoints-discontinuous",
                "topology.pcurve-locus-discontinuous",
                "topology.pcurve-global-fidelity-unproved",
                "topology.pcurve-association-ambiguous",
                "topology.pcurve-candidates-carrier-unresolved",
                "topology.face-multiple-outer-bounds",
                "topology.shell-disconnected-faces",
                "product.nauo-placement-unresolved",
                "product.nauo-placement-ambiguous",
                "product.body-conflicting-mapped-placements",
                "pmi.presentation-annotation-text-unordered",
                "pmi.presentation-annotation-placement-ambiguous",
                "pmi.dimensional-nominal-ambiguous",
                "pmi.dimensional-unnamed-measure-ambiguous",
                "pmi.length-unit-unresolved",
                "pmi.angle-unit-unresolved",
                "pmi.datum-system-invalid",
                "pmi.zone-unresolved",
                "pmi.dimension-modifier-unresolved",
                "pmi.dimension-kind-quantity-mismatch",
                "pmi.dimensional-limits-omitted",
                "presentation.conflicting-scalar-colors",
                "presentation.surface-side-invalid",
                "presentation.surface-transparency-conflict",
                "presentation.surface-transparency-misplaced",
                "presentation.context-dependent-style-unresolved",
                "drawing.record-too-few-parameters",
                "drawing.order-unstatable",
                "drawing.relationship-untyped-target",
                "drawing.relationship-target-ambiguous",
                "drawing.sheet-revision-unresolved",
                "drawing.revision-sheet-unresolved",
                "drawing.draughting-semantic-definition-untyped",
                "drawing.draughting-associated-item-untyped",
                "tessellation.item-body-unresolved",
                "tessellation.invalid-payload",
                "tessellation.item-undeclared",
                "tessellation.placement-unresolved",
                "tessellation.placement-ambiguous",
                "validation.measure-unit-unresolved",
                "validation.item-unsupported",
                "export.no-exportable-solids",
                "layer.item-without-carrier",
                "layer.hierarchy-flattened",
                "occurrence.unresolved-parent",
                "occurrence.no-local-product",
                "occurrence.placement-not-rigid",
                "body.non-rigid-transform",
                "body.hidden-visibility-unsupported",
                "presentation.hidden-appearance-visibility-unsupported",
                "presentation.hidden-layer-visibility-unsupported",
                "pmi.hidden-visibility-unsupported",
                "topology.wire-shell-free-vertices",
                "product.body-without-representation",
                "topology.surface-shell-wire-omitted",
                "tessellation.requires-ap242",
                "tessellation.feature-edges",
                "tessellation.corner-normals",
                "tessellation.triangle-groups",
                "tessellation.texture-assignments",
                "tessellation.invalid-cardinality",
                "tessellation.body-link-unwritable",
                "tessellation.metadata-reduced",
                "topology.unreachable-from-region",
                "geometry.analytic-surface-normalized",
                "geometry.elliptical-cone-reduced",
                "geometry.curveless-edge-omitted",
                "geometry.unknown-surface-face-omitted",
                "geometry.carrier-not-written",
                "pcurve.carrier-unwritable",
                "pcurve.on-fitted-helix-surface",
                "assembly.occurrence-omitted-no-parent-product",
                "topology.region-no-shell-list",
                "topology.wire-region-no-connected-edge-set",
                "topology.wire-region-missing-shell",
                "body.hidden-omitted",
                "presentation.hidden-layer-omitted",
                "appearance.binding-missing-asset",
                "appearance.binding-no-base-color",
                "appearance.binding-target-conflict",
                "pcurve.coedge-no-geometry",
                "pcurve.coedge-native-metadata",
                "pcurve.use-native-metadata",
                "topology.coedge-use-curve-not-represented",
                "topology.metadata-not-represented",
                "geometry.subd-omitted",
                "design.parametric-records-omitted",
                "presentation.drawing-records-omitted",
                "pmi.semantic-annotation-omitted",
                "asset.document-omitted",
                "assembly.joint-omitted",
                "product.non-part-kind",
                "product.bom-property-omitted",
                "occurrence.external-product",
                "occurrence.no-writable-product",
                "occurrence.metadata-omitted",
                "product.instance-definition-split",
                "product.instance-path-unresolved",
                "product.instance-prototype-bodies-omitted",
                "pmi.annotation-not-written",
                "pmi.all-around-components-unresolved",
                "source.association-omitted",
                "appearance.display-color-unstyled",
                "appearance.wire-body-transparency-omitted",
                "appearance.reduced-to-base-color",
                "appearance.binding-metadata-reduced",
                "attribute.source-record-not-written",
                "geometry.procedural-reduced-to-carrier",
                "geometry.helix-fitted-to-bspline",
                "geometry.procedural-sampled-to-bspline",
                "geometry.helix-fit-failed",
                "geometry.procedural-definition-not-written",
                "topology.region-no-writable-outer-shell",
                "topology.region-omitted-void-shell",
                "topology.region-solid-reduced-to-sheet",
                "topology.region-vertices-recovered",
                "topology.region-edges-recovered",
                "export.no-geometry-written",
                "geometry.edge-pcurve-agreement-sampled",
                "geometry.edge-pcurve-sampled-to-bspline",
                "topology.shell-omitted-outer-face",
                "topology.shell-omitted-unclassified-face",
                "topology.face-omitted-outer-loop",
                "topology.face-omitted-inner-loop",
                "topology.face-omitted-unclassified-loop",
                "topology.face-no-writable-bounds",
                "topology.face-incomplete-bounds",
                "topology.face-implicit-outer-bound-unwritable",
                "topology.loop-record-missing",
                "topology.loop-vertex-missing",
                "topology.loop-coedge-record-missing",
                "topology.loop-edge-not-writable",
                "topology.loop-no-writable-edges",
                "topology.loop-duplicate-coedge",
                "topology.loop-coedge-missing-for-order",
                "topology.loop-edge-missing-for-order",
                "topology.edge-no-3d-curve",
                "topology.edge-curve-not-writable",
                "topology.edge-curve-unsupported",
                "topology.loop-no-continuous-ordering",
                "topology.edge-record-missing",
                "topology.edge-start-vertex-missing",
                "topology.edge-end-vertex-missing",
                "topology.edge-no-vertex",
                "parse.expansion-bounded",
                "occurrence.expansion-output-truncated",
                "occurrence.expansion-depth-truncated",
                "topology.non-manifold-edge-split",
                "topology.open-edge-unsplit",
                "topology.face-boundary-unstated",
                "geometry.edge-vertex-off-carrier",
                "geometry.angle-unit-factor-corrected",
                "geometry.length-unit-contradicted",
                "product.occurrence-placement-inferred",
                "csg.item-unresolved",
                "csg.plant-orientation-ambiguous",
                "csg.solid-unsynthesized",
                "csg.boolean-unevaluated",
                "csg.profile-gap-closed",
                "csg.design-unscoped",
                "geometry.torus-sheet-unresolved",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in StepLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').expect("family.detail shape");
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(
                text.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'.'
                    || b == b'-'),
                "code {text} is not lowercase kebab"
            );
        }
    }

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in StepLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }

    /// The admission warning reports transferred source data and its
    /// verification status, so strict decode keeps it.
    #[test]
    fn globally_unproved_pcurve_admission_does_not_refuse_strict_decode() {
        let note = StepLossCode::PcurveGlobalFidelityUnproved.note("x");
        assert_eq!(note.severity, Severity::Warning);
        let pinned = Strictness {
            pinned: true,
            ..Strictness::default()
        };
        assert!(!pinned.refuses(&note, false));
    }

    /// `step.md` states a strict floor for a repaired noncanonical record
    /// and for no other code.
    #[test]
    fn only_a_repaired_noncanonical_record_pins_a_strict_floor() {
        let pinned = Strictness {
            pinned: true,
            ..Strictness::default()
        };
        for code in StepLossCode::ALL {
            let expected = match code {
                StepLossCode::ParseNoncanonicalSyntax => Some(Severity::Warning),
                _ => None,
            };
            assert_eq!(code.kind().strict_floor(), expected, "{}", code.code());
        }
        assert!(pinned.refuses(&StepLossCode::ParseNoncanonicalSyntax.note("x"), false));
        let substituted = StepLossCode::UncertaintyLengthAmbiguous.note("x");
        assert_eq!(substituted.severity, Severity::Warning);
        assert!(!pinned.refuses(&substituted, false));
    }
}
