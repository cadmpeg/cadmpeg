// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.3dm` decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`RhinoLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`RhinoLossCode::note`] is the single construction path for a
//! [`LossNote`] in this crate: it fixes the shared loss category and the
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `rhino` namespace.
//!
//! [`RhinoLossCode::shared_taxonomy`] is an exhaustive match with no fall-through
//! arm. A default arm would silently assign a category to a code added later,
//! and the categories this codec spans (geometry, annotation, attribute,
//! diagnostic) have no honest common default.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNote, LossTaxonomy},
    Severity,
};

/// One decode diagnostic: its message and, when the producer knows it, its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RhinoDiagnostic {
    /// The loss code the producer assigned, or `None` when the channel decides.
    pub(crate) code: Option<RhinoLossCode>,
    /// The human-readable message.
    pub(crate) message: String,
}

/// The decode diagnostic channel: messages carrying the code their producer knew.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Diagnostics(Vec<RhinoDiagnostic>);

impl Diagnostics {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    /// Records one classified diagnostic.
    pub(crate) fn push_coded(
        &mut self,
        code: impl Into<Option<RhinoLossCode>>,
        message: impl std::fmt::Display,
    ) {
        self.0.push(RhinoDiagnostic {
            code: code.into(),
            message: message.to_string(),
        });
    }

    /// Records a diagnostic whose category the consuming channel decides.
    pub(crate) fn push(&mut self, message: impl std::fmt::Display) {
        self.push_coded(None, message);
    }

    pub(crate) fn messages(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|entry| entry.message.as_str())
    }

    /// Places `earlier` ahead of the diagnostics already recorded.
    pub(crate) fn prepend(&mut self, earlier: Self) {
        self.0.splice(0..0, earlier.0);
    }

    /// Moves every diagnostic of `other` after the recorded ones.
    pub(crate) fn append(&mut self, other: &mut Self) {
        self.0.append(&mut other.0);
    }

    /// Adds a source label to each diagnostic of `other`.
    pub(crate) fn append_prefixed(&mut self, other: Self, prefix: std::fmt::Arguments<'_>) {
        for diagnostic in other {
            self.push_coded(
                diagnostic.code,
                format_args!("{prefix}: {}", diagnostic.message),
            );
        }
    }
}

impl std::ops::Deref for Diagnostics {
    type Target = [RhinoDiagnostic];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::Deref for RhinoDiagnostic {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.message
    }
}

impl Extend<RhinoDiagnostic> for Diagnostics {
    fn extend<T: IntoIterator<Item = RhinoDiagnostic>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl FromIterator<RhinoDiagnostic> for Diagnostics {
    fn from_iter<T: IntoIterator<Item = RhinoDiagnostic>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for Diagnostics {
    type Item = RhinoDiagnostic;
    type IntoIter = std::vec::IntoIter<RhinoDiagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a RhinoDiagnostic;
    type IntoIter = std::slice::Iter<'a, RhinoDiagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// A stable, machine-readable identifier for one `.3dm` transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`RhinoLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum RhinoLossCode {
    /// Container or table scan surfaced a structural diagnostic.
    ContainerScanDiagnostic,
    /// A stored checksum does not match the protected bytes.
    IntegrityFailure,
    /// A framed presentation record could not be transferred.
    PresentationRecordDropped,
    /// A typed native record has no admitted neutral projection.
    NativeRecordUnprojected,
    /// A framed annotation record could not be transferred.
    AnnotationRecordDropped,
    /// A typed annotation has no constructible neutral semantic projection.
    AnnotationRecordUnprojected,
    /// A neutral annotation cannot be encoded as a Rhino object.
    WriterAnnotationOmitted,
    /// A multi-line annotation text is joined into one native string.
    WriterAnnotationTextJoined,
    /// V1 annotation fields are omitted during conversion.
    WriterV1AnnotationReduced,
    /// A dimension value is derived from its definition points.
    WriterDimensionValueReduced,
    /// The document angular tolerance is reduced to the native limit.
    WriterAngularToleranceReduced,
    /// A framed product occurrence could not be transferred to the native graph.
    ProductOccurrenceDropped,
    /// A fully typed product record has no constructible neutral counterpart.
    ProductRecordUnprojected,
    /// A recognized annotation userdata payload could not be typed.
    AnnotationUserdataDropped,
    /// Viewport userdata has no typed CADIR owner.
    ViewportUserdataDropped,
    /// Mesh n-gon grouping is not represented in neutral tessellation.
    MeshNgonGroupingDropped,
    /// A stored enumeration value was retained but could not select a neutral value.
    EnumerationValueDegraded,
    /// A redundant count or size was inconsistent; dependent data was dropped.
    RedundantFieldRepaired,
    /// A Brep display-mesh cache slot was wrong-class or unreadable.
    BrepMeshCacheDegraded,
    /// A duplicate source record was resolved by the format's ownership rule.
    DuplicateRecordResolved,
    /// A stored quad was converted to neutral triangles.
    MeshQuadTopologyTriangulated,
    /// Embedded history geometry could not be decoded.
    HistoryEmbeddedGeometryDropped,
    /// A dimension-style override object was not applied.
    DimensionOverrideDropped,
    /// A history dependency points to a later producer and cannot enter ordered IR.
    HistoryDependencyDropped,
    /// Instance-definition records were malformed, ambiguous, or checksum-degraded.
    ContainerInstanceDefinitionDegraded,
    /// Census of object records framed against object records transferred.
    ObjectRecordCensus,
    /// An object class is not decoded; its records carry only retained bytes.
    ObjectFamilyNotTransferred,
    /// Object attributes (name, layer, color, visibility) did not transfer whole.
    ObjectAttributesDegraded,
    /// A framed object record could not be decoded from its payload.
    ObjectFramingUndecodable,
    /// A decode phase surfaced a per-record diagnostic.
    ObjectDecodeDiagnostic,
    /// A discontinuous polycurve join moved both source endpoints to their midpoint.
    PolycurveJoinGap,
    /// One B-rep trim lost its parameter-space curve while its topology remained.
    TrimPcurveDropped,
    /// B-rep topology fell back to a carrier-only transfer.
    TopologyBrepFallback,
    /// Hatch fill pattern is retained as a native pattern index, not a filled region.
    HatchFillNotTransferred,
    /// Polyedge segment references are retained without resolved edge identities.
    PolyedgeReferencesNotResolved,
    /// Trivariate cage control lattice is retained as text, not a typed deformation.
    CageLatticeNotTransferred,
    /// Space-morph deformation is retained as native parameters, not applied.
    MorphDeformationNotApplied,
    /// Curve-on-surface trim binding is retained as native parameters.
    CurveOnSurfaceBindingNotTransferred,
    /// A dimension style reference does not resolve to a decoded style record.
    DimensionStyleUnresolved,
    /// A dimension detail-view reference does not resolve to a decoded view.
    DimensionDetailReferenceUnresolved,
    /// A definition member or captive object UUID does not resolve to one record.
    ReferenceMemberUnresolved,
    /// A definition member or captive object UUID resolves to several records.
    ReferenceMemberAmbiguous,
    /// History-record geometry is retained without a neutral carrier.
    HistoryGeometryNotTransferred,
    /// Standalone mesh vertices are written at reduced (f32) precision.
    MeshVertexPrecisionReduced,
    /// Mesh normals are written at reduced (f32) precision.
    MeshNormalPrecisionReduced,
    /// Standalone support points and curves cannot keep their dependency.
    WriterSupportGeometryWithheld,
    /// A document arena has no 3DM representation.
    WriterDesignOmitted,
    /// Subdivision records have no writer encoding.
    WriterSubdOmitted,
    /// Procedural definitions are reduced to solved carriers.
    WriterProceduralReduced,
    /// An absent model scope is represented by an empty model.
    WriterScopeExpanded,
    /// An unspecified pcurve reversal is written as the native false value.
    WriterPcurveReversalDefaulted,
    /// An edge carrier is resolved through wrappers to an explicit leaf.
    WriterCarrierResolved,
    /// Material records have no writer encoding.
    WriterMaterialOmitted,
    /// Material texture channels cannot be written.
    WriterMaterialTextureOmitted,
    /// A material identity is changed to keep physical components distinct.
    WriterMaterialUuidReassigned,
    /// Attribute records have no writer encoding.
    WriterAttributesOmitted,
    /// A dimension style archive index changes.
    WriterDimensionStyleIndexReassigned,
    /// A text style has no writable native record.
    WriterTextStyleOmitted,
    /// A text style reference is reduced or reassigned.
    WriterTextStyleReferenceReduced,
    /// A native light has incomplete or unrepresentable fields.
    WriterLightOmitted,
    /// A light's stored archive index changes to its emitted table position.
    WriterLightIndexReassigned,
    /// A light attribute reference uses a reassigned table index.
    WriterLightAttributeIndexReassigned,
    /// A hatch pattern has incomplete or unrepresentable fields.
    WriterHatchPatternOmitted,
    /// Hatch pattern fields cannot be retained by the selected table layout.
    WriterHatchPatternFieldsReduced,
    /// A linetype has no writable record.
    WriterLinetypeOmitted,
    /// A linetype archive index changes.
    WriterLinetypeReduced,
    /// A layer linetype reference changes its archive index or is omitted.
    WriterLayerLinetypeReduced,
    /// A layer material reference changes its archive index or is omitted.
    WriterLayerMaterialReduced,
    /// A native layer color cannot be represented.
    WriterLayerColorReduced,
    /// Layer membership cannot be assigned to an individual non-object record.
    WriterLayerMembershipOmitted,
    /// An unspecified layer visibility is written as the native default.
    WriterLayerVisibilityDefaulted,
    /// A layer description is normalized by the native format.
    WriterLayerDescriptionNormalized,
    /// Appearance binding change-version channels have no 3DM carrier.
    WriterAppearanceChannelsOmitted,
    /// A face appearance binding is reduced to a color or omitted.
    WriterFaceAppearanceReduced,
    /// Appearance metadata has no matching 3DM material fields.
    WriterAppearanceMetadataOmitted,
    /// Edge or vertex appearance bindings lose their topology target.
    WriterSubobjectAppearanceReduced,
    /// Per-point appearance bindings lose their material link in a point cloud.
    WriterPointCloudAppearanceReduced,
    /// A free vertex loses its tolerance or its point's dependency role.
    WriterFreeVertexMetadataReduced,
    /// A point and vertex are reidentified as members of a point cloud.
    WriterPointCloudIdentityReduced,
    /// A point's display assignment replaces its containing body's color.
    WriterBodyColorReduced,
    /// A shared pcurve needs different native carriers for its used intervals.
    WriterPcurveExpanded,
    /// A pcurve is reparameterized to follow its trim direction.
    WriterPcurveDirectionReversed,
    /// An unbounded line pcurve is restricted to its used trim intervals.
    WriterPcurveDomainRestricted,
    /// An infinite plane carrier is represented with a finite native domain.
    WriterPlaneDomainRestricted,
    /// Object display colors were quantized to native byte channels.
    WriterColorPrecisionReduced,
    /// An entity has no writable carrier.
    WriterEntityOmitted,
    /// A body is retained with only the topology its carrier can hold.
    WriterBodyReduced,
    /// A body cannot be encoded without its unsupported topology.
    WriterBodyOmitted,
    /// A body placement cannot be represented in the native carrier.
    WriterBodyTransformNotApplied,
    /// A finite body placement cannot be applied to its geometry.
    WriterBodyPlacementOmitted,
    /// No geometry can be written to the target.
    NoExportableSolids,
    /// Different coedge pcurves were compared at discrete parameters.
    EdgePcurveAgreementSampled,
    /// A recovered edge was fitted at sampled parameters.
    EdgePcurveSampledToBspline,
    /// A field was read under the legacy reading because the archive carries no
    /// openNURBS writer-version stamp to verify that record against.
    ///
    /// Per record, inside a dialect the archive word already identified and
    /// whose grammar this codec verified. It is deliberately *not*
    /// `source.dialect-unverified`: the other codecs pin that string for the
    /// document-level statement "the dialect itself was not verified", which
    /// holds exactly when `Admission::Residual` is reported. Rhino
    /// charges this one inside `Admission::Admitted` documents, so a consumer
    /// joining loss code to admission state must be able to tell them apart.
    /// The taxonomy family below is still the right one.
    SourceWriterStampUnverified,
    /// The archive-version word is outside the declared set, so the document
    /// was read by the residual chunk-width route without a declared identity.
    ///
    /// Document level, and the counterpart of the per-record code above:
    /// charged exactly when the primary-layer `crate::dialect` match is
    /// `Admission::Residual`, from the same predicate that decides
    /// the admission. A word no row claims still selects its own chunk width
    /// and checksum mechanics. No declared archive row is substituted, and
    /// nothing verified that the observed word means those mechanics.
    SourceDialectUnverified,
    /// The selected write target differs from the same-format source dialect.
    SourceDialectDisplaced,
    /// Body kind came from the closed-shell gauge or from an unverified stored
    /// solid flag rather than from a flag the writer stamp vouches for.
    TopologyBodyKindGaugeSubstituted,
}

impl RhinoLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [RhinoLossCode] = &[
        Self::ContainerScanDiagnostic,
        Self::IntegrityFailure,
        Self::PresentationRecordDropped,
        Self::NativeRecordUnprojected,
        Self::AnnotationRecordDropped,
        Self::AnnotationRecordUnprojected,
        Self::WriterAnnotationOmitted,
        Self::WriterAnnotationTextJoined,
        Self::WriterV1AnnotationReduced,
        Self::WriterDimensionValueReduced,
        Self::WriterAngularToleranceReduced,
        Self::ProductOccurrenceDropped,
        Self::ProductRecordUnprojected,
        Self::AnnotationUserdataDropped,
        Self::ViewportUserdataDropped,
        Self::MeshNgonGroupingDropped,
        Self::EnumerationValueDegraded,
        Self::RedundantFieldRepaired,
        Self::BrepMeshCacheDegraded,
        Self::DuplicateRecordResolved,
        Self::MeshQuadTopologyTriangulated,
        Self::HistoryEmbeddedGeometryDropped,
        Self::DimensionOverrideDropped,
        Self::HistoryDependencyDropped,
        Self::ContainerInstanceDefinitionDegraded,
        Self::ObjectRecordCensus,
        Self::ObjectFamilyNotTransferred,
        Self::ObjectAttributesDegraded,
        Self::ObjectFramingUndecodable,
        Self::ObjectDecodeDiagnostic,
        Self::PolycurveJoinGap,
        Self::TrimPcurveDropped,
        Self::TopologyBrepFallback,
        Self::HatchFillNotTransferred,
        Self::PolyedgeReferencesNotResolved,
        Self::CageLatticeNotTransferred,
        Self::MorphDeformationNotApplied,
        Self::CurveOnSurfaceBindingNotTransferred,
        Self::DimensionStyleUnresolved,
        Self::DimensionDetailReferenceUnresolved,
        Self::ReferenceMemberUnresolved,
        Self::ReferenceMemberAmbiguous,
        Self::HistoryGeometryNotTransferred,
        Self::MeshVertexPrecisionReduced,
        Self::MeshNormalPrecisionReduced,
        Self::WriterSupportGeometryWithheld,
        Self::WriterDesignOmitted,
        Self::WriterSubdOmitted,
        Self::WriterProceduralReduced,
        Self::WriterScopeExpanded,
        Self::WriterPcurveReversalDefaulted,
        Self::WriterCarrierResolved,
        Self::WriterMaterialOmitted,
        Self::WriterMaterialTextureOmitted,
        Self::WriterMaterialUuidReassigned,
        Self::WriterAttributesOmitted,
        Self::WriterDimensionStyleIndexReassigned,
        Self::WriterTextStyleOmitted,
        Self::WriterTextStyleReferenceReduced,
        Self::WriterLightOmitted,
        Self::WriterLightIndexReassigned,
        Self::WriterLightAttributeIndexReassigned,
        Self::WriterHatchPatternOmitted,
        Self::WriterHatchPatternFieldsReduced,
        Self::WriterLinetypeOmitted,
        Self::WriterLinetypeReduced,
        Self::WriterLayerLinetypeReduced,
        Self::WriterLayerMaterialReduced,
        Self::WriterLayerColorReduced,
        Self::WriterLayerMembershipOmitted,
        Self::WriterLayerVisibilityDefaulted,
        Self::WriterLayerDescriptionNormalized,
        Self::WriterAppearanceChannelsOmitted,
        Self::WriterFaceAppearanceReduced,
        Self::WriterAppearanceMetadataOmitted,
        Self::WriterSubobjectAppearanceReduced,
        Self::WriterPointCloudAppearanceReduced,
        Self::WriterFreeVertexMetadataReduced,
        Self::WriterPointCloudIdentityReduced,
        Self::WriterBodyColorReduced,
        Self::WriterPcurveExpanded,
        Self::WriterPcurveDirectionReversed,
        Self::WriterPcurveDomainRestricted,
        Self::WriterPlaneDomainRestricted,
        Self::WriterColorPrecisionReduced,
        Self::WriterEntityOmitted,
        Self::WriterBodyReduced,
        Self::WriterBodyOmitted,
        Self::WriterBodyTransformNotApplied,
        Self::WriterBodyPlacementOmitted,
        Self::NoExportableSolids,
        Self::EdgePcurveAgreementSampled,
        Self::EdgePcurveSampledToBspline,
        Self::SourceWriterStampUnverified,
        Self::SourceDialectUnverified,
        Self::SourceDialectDisplaced,
        Self::TopologyBodyKindGaugeSubstituted,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::ContainerScanDiagnostic => "container.scan-diagnostic",
            Self::IntegrityFailure => "container.integrity-failure",
            Self::PresentationRecordDropped => "presentation.record-dropped",
            Self::NativeRecordUnprojected => "record.neutral-unprojected",
            Self::AnnotationRecordDropped => "annotation.record-dropped",
            Self::AnnotationRecordUnprojected => "annotation.record-unprojected",
            Self::WriterAnnotationOmitted => "writer.annotation-omitted",
            Self::WriterAnnotationTextJoined => "writer.annotation-text-joined",
            Self::WriterV1AnnotationReduced => "writer.legacy-annotation-reduced",
            Self::WriterDimensionValueReduced => "writer.dimension-value-reduced",
            Self::WriterAngularToleranceReduced => "writer.angular-tolerance-reduced",
            Self::ProductOccurrenceDropped => "product.occurrence-dropped",
            Self::ProductRecordUnprojected => "product.record-unprojected",
            Self::AnnotationUserdataDropped => "annotation.userdata-dropped",
            Self::ViewportUserdataDropped => "viewport.userdata-dropped",
            Self::MeshNgonGroupingDropped => "mesh.ngon-grouping-dropped",
            Self::EnumerationValueDegraded => "container.enumeration-value-degraded",
            Self::RedundantFieldRepaired => "container.redundant-field-repaired",
            Self::BrepMeshCacheDegraded => "brep.mesh-cache-degraded",
            Self::DuplicateRecordResolved => "container.duplicate-record-resolved",
            Self::MeshQuadTopologyTriangulated => "mesh.quad-topology-triangulated",
            Self::HistoryEmbeddedGeometryDropped => "history.embedded-geometry-dropped",
            Self::DimensionOverrideDropped => "dimension.override-dropped",
            Self::HistoryDependencyDropped => "history.dependency-dropped",
            Self::ContainerInstanceDefinitionDegraded => "container.instance-definition-degraded",
            Self::ObjectRecordCensus => "object.record-census",
            Self::ObjectFamilyNotTransferred => "object.family-not-transferred",
            Self::ObjectAttributesDegraded => "object.attributes-degraded",
            Self::ObjectFramingUndecodable => "object.framing-undecodable",
            Self::ObjectDecodeDiagnostic => "object.decode-diagnostic",
            Self::PolycurveJoinGap => "curve.polycurve-join-gap",
            Self::TrimPcurveDropped => "brep.trim-pcurve-dropped",
            Self::TopologyBrepFallback => "topology.brep-fallback",
            Self::HatchFillNotTransferred => "hatch.fill-not-transferred",
            Self::PolyedgeReferencesNotResolved => "polyedge.references-not-resolved",
            Self::CageLatticeNotTransferred => "cage.lattice-not-transferred",
            Self::MorphDeformationNotApplied => "morph.deformation-not-applied",
            Self::CurveOnSurfaceBindingNotTransferred => "curve-on-surface.binding-not-transferred",
            Self::DimensionStyleUnresolved => "dimension.style-unresolved",
            Self::DimensionDetailReferenceUnresolved => "dimension.detail-reference-unresolved",
            Self::ReferenceMemberUnresolved => "reference.member-unresolved",
            Self::ReferenceMemberAmbiguous => "reference.member-ambiguous",
            Self::HistoryGeometryNotTransferred => "history.geometry-not-transferred",
            Self::MeshVertexPrecisionReduced => "mesh.vertex-precision-reduced",
            Self::WriterSupportGeometryWithheld => "writer.support-geometry-withheld",
            Self::WriterDesignOmitted => "design.parametric-records-omitted",
            Self::WriterSubdOmitted => "writer.subds-omitted",
            Self::WriterProceduralReduced => "writer.procedural-reduced",
            Self::WriterScopeExpanded => "writer.scope-expanded",
            Self::WriterPcurveReversalDefaulted => "writer.pcurve-reversal-defaulted",
            Self::WriterCarrierResolved => "writer.carrier-resolved",
            Self::WriterMaterialOmitted => "writer.materials-omitted",
            Self::WriterMaterialTextureOmitted => "writer.material-texture-omitted",
            Self::WriterMaterialUuidReassigned => "writer.material-uuid-reassigned",
            Self::WriterAttributesOmitted => "writer.attributes-omitted",
            Self::WriterDimensionStyleIndexReassigned => "writer.dimension-style-index-reassigned",
            Self::WriterTextStyleOmitted => "writer.text-style-omitted",
            Self::WriterTextStyleReferenceReduced => "writer.text-style-reference-reduced",
            Self::WriterLightOmitted => "writer.light-omitted",
            Self::WriterLightIndexReassigned => "writer.light-index-reassigned",
            Self::WriterLightAttributeIndexReassigned => "writer.light-attribute-index-reassigned",
            Self::WriterHatchPatternOmitted => "writer.hatch-pattern-omitted",
            Self::WriterHatchPatternFieldsReduced => "writer.hatch-pattern-fields-reduced",
            Self::WriterLinetypeOmitted => "writer.linetype-omitted",
            Self::WriterLinetypeReduced => "writer.linetype-reduced",
            Self::WriterLayerLinetypeReduced => "writer.layer-linetype-reduced",
            Self::WriterLayerMaterialReduced => "writer.layer-material-reduced",
            Self::WriterLayerColorReduced => "writer.layer-color-reduced",
            Self::WriterLayerMembershipOmitted => "writer.layer-membership-omitted",
            Self::WriterLayerVisibilityDefaulted => "writer.layer-visibility-defaulted",
            Self::WriterLayerDescriptionNormalized => "writer.layer-description-normalized",
            Self::WriterAppearanceChannelsOmitted => "writer.appearance-channels-omitted",
            Self::WriterFaceAppearanceReduced => "writer.face-appearance-reduced",
            Self::WriterAppearanceMetadataOmitted => "writer.appearance-metadata-omitted",
            Self::WriterSubobjectAppearanceReduced => "writer.subobject-appearance-reduced",
            Self::WriterPointCloudAppearanceReduced => "writer.point-cloud-appearance-reduced",
            Self::WriterFreeVertexMetadataReduced => "writer.free-vertex-metadata-reduced",
            Self::WriterPointCloudIdentityReduced => "writer.point-cloud-identity-reduced",
            Self::WriterBodyColorReduced => "writer.body-color-reduced",
            Self::WriterPcurveExpanded => "writer.pcurve-expanded",
            Self::WriterPcurveDirectionReversed => "writer.pcurve-direction-reversed",
            Self::WriterPcurveDomainRestricted => "writer.pcurve-domain-restricted",
            Self::WriterPlaneDomainRestricted => "writer.plane-domain-restricted",
            Self::WriterColorPrecisionReduced => "appearance.color-precision-reduced",
            Self::WriterEntityOmitted => "writer.entity-omitted",
            Self::WriterBodyReduced => "writer.body-reduced",
            Self::WriterBodyOmitted => "writer.body-omitted",
            Self::WriterBodyTransformNotApplied => "body.non-rigid-transform",
            Self::WriterBodyPlacementOmitted => "body.placement-omitted",
            Self::NoExportableSolids => "export.no-exportable-solids",
            Self::EdgePcurveAgreementSampled => "geometry.edge-pcurve-agreement-sampled",
            Self::EdgePcurveSampledToBspline => "geometry.edge-pcurve-sampled-to-bspline",
            Self::MeshNormalPrecisionReduced => "mesh.normal-precision-reduced",
            Self::SourceWriterStampUnverified => "source.writer-stamp-unverified",
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::SourceDialectDisplaced => "target.source-dialect-displaced",
            Self::TopologyBodyKindGaugeSubstituted => "topology.body-kind-gauge-substituted",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::ObjectRecordCensus => Severity::Info,
            Self::ObjectFramingUndecodable
            | Self::IntegrityFailure
            | Self::WriterEntityOmitted
            | Self::WriterBodyReduced
            | Self::WriterBodyOmitted => Severity::Error,
            Self::WriterAnnotationOmitted
            | Self::WriterLightOmitted
            | Self::WriterLinetypeOmitted
            | Self::WriterHatchPatternOmitted => Severity::Error,
            _ => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ContainerScanDiagnostic
            | Self::ContainerInstanceDefinitionDegraded
            | Self::ObjectFramingUndecodable
            | Self::ObjectDecodeDiagnostic
            | Self::AnnotationRecordDropped
            | Self::AnnotationUserdataDropped
            | Self::PolycurveJoinGap
            | Self::ReferenceMemberUnresolved
            | Self::ReferenceMemberAmbiguous => LossTaxonomy::DecodeDiagnostic,
            Self::TrimPcurveDropped => LossTaxonomy::PcurveOmitted,
            Self::IntegrityFailure => LossTaxonomy::IntegrityFailure,
            Self::PresentationRecordDropped => LossTaxonomy::AssetNotTransferred,
            Self::ProductRecordUnprojected => LossTaxonomy::MetadataNotTransferred,
            Self::NativeRecordUnprojected => LossTaxonomy::MetadataNotTransferred,
            Self::AnnotationRecordUnprojected => LossTaxonomy::MetadataNotTransferred,
            Self::WriterAnnotationOmitted => LossTaxonomy::PmiOmitted,
            Self::WriterAnnotationTextJoined
            | Self::WriterV1AnnotationReduced
            | Self::WriterDimensionValueReduced => LossTaxonomy::PmiOmitted,
            Self::ViewportUserdataDropped | Self::ProductOccurrenceDropped => {
                LossTaxonomy::RecordNotTyped
            }
            Self::MeshNgonGroupingDropped | Self::MeshQuadTopologyTriangulated => {
                LossTaxonomy::RecordNotTyped
            }
            Self::EnumerationValueDegraded | Self::RedundantFieldRepaired => {
                LossTaxonomy::DecodeDiagnostic
            }
            Self::BrepMeshCacheDegraded => LossTaxonomy::RecordNotTyped,
            Self::DuplicateRecordResolved => LossTaxonomy::DecodeDiagnostic,
            Self::HistoryEmbeddedGeometryDropped => LossTaxonomy::GeometryNotTransferred,
            Self::DimensionOverrideDropped => LossTaxonomy::PmiOmitted,
            Self::HistoryDependencyDropped => LossTaxonomy::ReferenceGraphNotClosed,
            Self::ObjectRecordCensus => LossTaxonomy::ObjectRecordsUntransferred,
            Self::ObjectFamilyNotTransferred => LossTaxonomy::UnsupportedObjectFamily,
            Self::ObjectAttributesDegraded => LossTaxonomy::AttributesNotTransferred,
            Self::TopologyBrepFallback => LossTaxonomy::TopologyNotTransferred,
            Self::HatchFillNotTransferred
            | Self::PolyedgeReferencesNotResolved
            | Self::CageLatticeNotTransferred
            | Self::MorphDeformationNotApplied
            | Self::CurveOnSurfaceBindingNotTransferred
            | Self::HistoryGeometryNotTransferred => LossTaxonomy::RecordNotTyped,
            Self::DimensionStyleUnresolved | Self::DimensionDetailReferenceUnresolved => {
                LossTaxonomy::PmiOmitted
            }
            Self::MeshVertexPrecisionReduced | Self::MeshNormalPrecisionReduced => {
                LossTaxonomy::MeshVertexPrecision
            }
            Self::SourceWriterStampUnverified | Self::SourceDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::WriterSupportGeometryWithheld
            | Self::WriterEntityOmitted
            | Self::WriterPlaneDomainRestricted => LossTaxonomy::GeometryNotTransferred,
            Self::WriterPcurveReversalDefaulted => LossTaxonomy::TopologyNotTransferred,
            Self::WriterBodyOmitted
            | Self::WriterBodyReduced
            | Self::WriterFreeVertexMetadataReduced
            | Self::WriterPointCloudIdentityReduced
            | Self::WriterPcurveExpanded
            | Self::WriterPcurveDirectionReversed
            | Self::WriterPcurveDomainRestricted => LossTaxonomy::TopologyNotTransferred,
            Self::WriterBodyTransformNotApplied | Self::WriterBodyPlacementOmitted => {
                LossTaxonomy::BodyTransformNotApplied
            }
            Self::WriterScopeExpanded => LossTaxonomy::MetadataNotTransferred,
            Self::WriterDesignOmitted => LossTaxonomy::ParametricRecordOmitted,
            Self::WriterSubdOmitted => LossTaxonomy::SubdOmitted,
            Self::WriterProceduralReduced | Self::WriterCarrierResolved => {
                LossTaxonomy::ProceduralReduced
            }
            Self::WriterMaterialOmitted
            | Self::WriterMaterialTextureOmitted
            | Self::WriterMaterialUuidReassigned
            | Self::WriterLinetypeOmitted
            | Self::WriterLinetypeReduced
            | Self::WriterLayerLinetypeReduced
            | Self::WriterLayerMaterialReduced
            | Self::WriterLayerColorReduced
            | Self::WriterLayerMembershipOmitted
            | Self::WriterLayerVisibilityDefaulted
            | Self::WriterLayerDescriptionNormalized
            | Self::WriterAppearanceChannelsOmitted
            | Self::WriterFaceAppearanceReduced
            | Self::WriterAppearanceMetadataOmitted
            | Self::WriterSubobjectAppearanceReduced
            | Self::WriterPointCloudAppearanceReduced => LossTaxonomy::AppearanceReduced,
            Self::WriterBodyColorReduced => LossTaxonomy::AppearanceReduced,
            Self::WriterAngularToleranceReduced => LossTaxonomy::MetadataNotTransferred,
            Self::WriterAttributesOmitted
            | Self::WriterDimensionStyleIndexReassigned
            | Self::WriterTextStyleOmitted
            | Self::WriterTextStyleReferenceReduced
            | Self::WriterLightOmitted
            | Self::WriterLightIndexReassigned
            | Self::WriterLightAttributeIndexReassigned
            | Self::WriterHatchPatternOmitted
            | Self::WriterHatchPatternFieldsReduced => LossTaxonomy::AttributesNotTransferred,
            Self::WriterColorPrecisionReduced => LossTaxonomy::AppearanceReduced,
            Self::NoExportableSolids => LossTaxonomy::NoExportableSolids,
            Self::EdgePcurveAgreementSampled | Self::EdgePcurveSampledToBspline => {
                LossTaxonomy::GeometryAgreementSampled
            }
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::TopologyBodyKindGaugeSubstituted => LossTaxonomy::TopologyGaugeSubstituted,
        }
    }

    /// Strict floor pinned from this local code (independent of taxonomy remap).
    ///
    /// Defaults to the taxonomy floor so a later local→taxonomy remap cannot
    /// silently change rejection. `ObjectFramingUndecodable` pins Warning above
    /// its `DecodeDiagnostic` taxonomy (which is otherwise tolerable).
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::IntegrityFailure | Self::ObjectFramingUndecodable => Some(Severity::Warning),
            Self::SourceWriterStampUnverified => None,
            other => other.shared_taxonomy().strict_floor(),
        }
    }

    /// Namespaced [`LossKind`] for this local code (taxonomy + pinned floor).
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        cadmpeg_ir::report::loss::NamespacedLossKind::new(
            const {
                match cadmpeg_ir::report::loss::LossNamespace::new("rhino") {
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

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `rhino/<local>`; the message is the per-instance
    /// text only. Severity and strict floor come from the local code.
    #[must_use]
    pub(crate) fn note(self, message: impl std::fmt::Display) -> LossNote {
        LossNote::new(self.kind(), message.to_string()).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::RhinoLossCode;
    use std::collections::BTreeSet;

    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = RhinoLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "container.scan-diagnostic",
                "container.integrity-failure",
                "presentation.record-dropped",
                "record.neutral-unprojected",
                "annotation.record-dropped",
                "annotation.record-unprojected",
                "writer.annotation-omitted",
                "writer.annotation-text-joined",
                "writer.legacy-annotation-reduced",
                "writer.dimension-value-reduced",
                "writer.angular-tolerance-reduced",
                "product.occurrence-dropped",
                "product.record-unprojected",
                "annotation.userdata-dropped",
                "viewport.userdata-dropped",
                "mesh.ngon-grouping-dropped",
                "container.enumeration-value-degraded",
                "container.redundant-field-repaired",
                "brep.mesh-cache-degraded",
                "container.duplicate-record-resolved",
                "mesh.quad-topology-triangulated",
                "history.embedded-geometry-dropped",
                "dimension.override-dropped",
                "history.dependency-dropped",
                "container.instance-definition-degraded",
                "object.record-census",
                "object.family-not-transferred",
                "object.attributes-degraded",
                "object.framing-undecodable",
                "object.decode-diagnostic",
                "curve.polycurve-join-gap",
                "brep.trim-pcurve-dropped",
                "topology.brep-fallback",
                "hatch.fill-not-transferred",
                "polyedge.references-not-resolved",
                "cage.lattice-not-transferred",
                "morph.deformation-not-applied",
                "curve-on-surface.binding-not-transferred",
                "dimension.style-unresolved",
                "dimension.detail-reference-unresolved",
                "reference.member-unresolved",
                "reference.member-ambiguous",
                "history.geometry-not-transferred",
                "mesh.vertex-precision-reduced",
                "mesh.normal-precision-reduced",
                "writer.support-geometry-withheld",
                "design.parametric-records-omitted",
                "writer.subds-omitted",
                "writer.procedural-reduced",
                "writer.scope-expanded",
                "writer.pcurve-reversal-defaulted",
                "writer.carrier-resolved",
                "writer.materials-omitted",
                "writer.material-texture-omitted",
                "writer.material-uuid-reassigned",
                "writer.attributes-omitted",
                "writer.dimension-style-index-reassigned",
                "writer.text-style-omitted",
                "writer.text-style-reference-reduced",
                "writer.light-omitted",
                "writer.light-index-reassigned",
                "writer.light-attribute-index-reassigned",
                "writer.hatch-pattern-omitted",
                "writer.hatch-pattern-fields-reduced",
                "writer.linetype-omitted",
                "writer.linetype-reduced",
                "writer.layer-linetype-reduced",
                "writer.layer-material-reduced",
                "writer.layer-color-reduced",
                "writer.layer-membership-omitted",
                "writer.layer-visibility-defaulted",
                "writer.layer-description-normalized",
                "writer.appearance-channels-omitted",
                "writer.face-appearance-reduced",
                "writer.appearance-metadata-omitted",
                "writer.subobject-appearance-reduced",
                "writer.point-cloud-appearance-reduced",
                "writer.free-vertex-metadata-reduced",
                "writer.point-cloud-identity-reduced",
                "writer.body-color-reduced",
                "writer.pcurve-expanded",
                "writer.pcurve-direction-reversed",
                "writer.pcurve-domain-restricted",
                "writer.plane-domain-restricted",
                "appearance.color-precision-reduced",
                "writer.entity-omitted",
                "writer.body-reduced",
                "writer.body-omitted",
                "body.non-rigid-transform",
                "body.placement-omitted",
                "export.no-exportable-solids",
                "geometry.edge-pcurve-agreement-sampled",
                "geometry.edge-pcurve-sampled-to-bspline",
                "source.writer-stamp-unverified",
                "source.dialect-unverified",
                "target.source-dialect-displaced",
                "topology.body-kind-gauge-substituted",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in RhinoLossCode::ALL {
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

    /// The note builder fixes severity from the code and emits the code string.
    #[test]
    fn note_takes_severity_from_the_code_and_renders_it() {
        for code in RhinoLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert_eq!(note.code.namespace(), "rhino");
            assert_eq!(note.code.local_code(), code.code());
            assert!(note.provenance.is_none());
        }
    }

    #[test]
    fn missing_writer_stamp_does_not_trigger_document_dialect_strictness() {
        assert_eq!(
            RhinoLossCode::SourceWriterStampUnverified
                .kind()
                .strict_floor(),
            None
        );
        assert_eq!(
            RhinoLossCode::SourceDialectUnverified.kind().strict_floor(),
            Some(cadmpeg_ir::report::Severity::Warning)
        );
    }
}
