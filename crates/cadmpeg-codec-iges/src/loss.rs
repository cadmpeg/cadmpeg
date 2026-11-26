// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for IGES decoding and writing.
//!
//! Every fallback, approximation, and drop the codec reports carries a stable
//! machine-readable code from [`IgesLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`IgesLossCode::note`] is the single construction path for a
//! [`LossNote`] in this crate: it fixes the shared loss category and the
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `iges` namespace.
//!
//! [`IgesLossCode::shared_taxonomy`] is an exhaustive match with no fall-through
//! arm. A default arm would silently assign a category to a code added later,
//! and the categories this codec spans (decode, entity, graph, presentation,
//! geometry, writer) have no honest common default.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNote, LossTaxonomy},
    Severity,
};
macro_rules! loss_codes {
    ($( $(#[$meta:meta])* $variant:ident => $code:literal ),+ $(,)?) => {
        /// A stable identifier for one IGES transfer loss.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) enum IgesLossCode {
            $( $(#[$meta])* $variant ),+
        }

        impl IgesLossCode {
            /// Every code in declaration order.
            #[cfg(test)]
            const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The stable string identifier.
            #[must_use]
            pub(crate) const fn code(self) -> &'static str {
                match self { $(Self::$variant => $code),+ }
            }
        }
    };
}

loss_codes! {
    /// A Directory presentation or naming field remains source-only.
    DirectoryMetadataUnreadable => "directory.metadata-unreadable",
    /// A readable display field violates a source rule without changing geometry.
    DirectoryMetadataNoncanonical => "directory.metadata-noncanonical",
    /// Product-occurrence expansion stopped at the configured output limit.
    OccurrenceExpansionOutputTruncated => "occurrence.expansion-output-truncated",
    /// Product-occurrence expansion stopped at the configured nesting-depth limit.
    OccurrenceExpansionDepthTruncated => "occurrence.expansion-depth-truncated",
    /// Product-occurrence root inference was suppressed by a malformed member list.
    OccurrenceRootInferenceBlocked => "occurrence.root-inference-blocked",
    /// Product-occurrence expansion omitted an instance or member with malformed placement data.
    OccurrencePlacementMalformed => "occurrence.placement-malformed",
    /// An envelope-admitted entity was retained without a neutral projection.
    EntityRetainedUnprojected => "entity.retained-unprojected",
    /// An entity type/form is outside the Fixed ASCII mechanical/document envelope.
    EntityOutsideEnvelope => "entity.outside-envelope",
    /// An entity was not projected; the instance message names the reason.
    EntityNotProjected => "entity.not-projected",
    /// A recognized geometric entity could not retain its required geometry.
    GeometryNotProjected => "geometry.not-projected",
    /// A framed Parameter Data literal is retained without an interpreted value.
    ParameterLiteralUnusable => "parameter.literal-unusable",
    /// Redundant spline claims disagreed with the serialized carrier mathematics.
    SplineClaimRecovered => "geometry.spline-claim-recovered",
    /// A NURBS coordinate or parameter transformation produced a non-finite value.
    NurbsTransformNonFinite => "geometry.nurbs-transform-non-finite",
    /// A boundary pcurve leaves the finite parameter domain of its support surface.
    BoundaryPcurveOutsideSupportDomain => "topology.boundary-pcurve-outside-support-domain",
    /// An alternate boundary pcurve does not compose its model-space curve.
    BoundaryPcurveDisagrees => "topology.boundary-pcurve-disagrees",
    /// A Type 144 boundary has no winding about its support normal and keeps its source traversal.
    BoundaryWindingUnresolved => "topology.boundary-winding-unresolved",
    /// A face its source holds with no boundary on a plane, cylinder, or cone,
    /// whose domain is unbounded, is stated partial with no loop.
    TopologyFaceBoundaryUnstated => "topology.face-boundary-unstated",
    /// A Directory Entry pointer did not resolve to the expected target.
    PointerUnresolved => "graph.pointer-unresolved",
    /// Parameter Data has more than one structural trailing pointer-group boundary.
    ParameterBoundaryAmbiguous => "parameter.boundary-ambiguous",
    /// A counted list declares more items than its Parameter Data record holds.
    ParameterCountOverdeclared => "parameter.count-overdeclared",
    /// A Type 422 attribute table states a count no row grid can be read from.
    AttributeTableCountUnstatable => "parameter.attribute-table-count-unstatable",
    /// One Directory Entry record kept its raw cards because its typed fields were not recovered.
    DirectoryRecordQuarantined => "directory.record-quarantined",
    /// A Compressed ASCII Data record restated an earlier Directory sequence and is not stated.
    DirectoryRecordRepeated => "directory.record-repeated",
    /// One entity's Parameter Data kept its raw cards because its tokens were not recovered.
    ParameterDataQuarantined => "parameter.data-quarantined",
    /// Card framing came from the card census because the file's own declaration did not.
    CardFramingRecovered => "card.framing-recovered",
    /// Directory display, font, or color data was not projected.
    DisplayDataNotProjected => "presentation.display-data-not-projected",
    /// A blanked carrier that no topology uses states no color, so no
    /// appearance binding states its blank status.
    FreeGeometryBlankStatusNotStated => "presentation.free-geometry-blank-status-not-stated",
    /// A drawing has conflicting valid properties of the same form.
    DrawingPropertyAmbiguous => "presentation.drawing-property-ambiguous",
    /// A body owner has conflicting valid Type 406 Form 15 names.
    BodyNameAmbiguous => "presentation.body-name-ambiguous",
    /// The Global line-weight scale is unavailable, so no entity has a width.
    LineWeightScaleUnavailable => "presentation.line-weight-scale-unavailable",
    /// An unusable Type 118 developability assertion did not change its rails.
    RuledDevelopabilityRecovered => "geometry.ruled-developability-recovered",
    /// A Type 118 developability flag was not transferred to neutral geometry.
    RuledDevelopabilityNotTransferred => "geometry.ruled-developability-not-transferred",
    /// Type 112 or Type 114 header semantics were not transferred to neutral geometry.
    SplineHeaderNotTransferred => "geometry.spline-header-not-transferred",
    /// A Type 102 composite has no admitted concatenated carrier.
    CompositeCarrierDegraded => "curve.composite-carrier-degraded",
    /// A Global metadata field is absent or malformed and was not transferred.
    GlobalMetadataFieldUnusable => "global.metadata-field-unusable",
    /// A Global comparison context came from this codec's specification.
    GlobalSemanticContextSubstituted => "global.semantic-context-substituted",
    /// A Global real used a recoverable noncanonical numeric spelling.
    GlobalNumericSyntaxRecovered => "global.numeric-syntax-recovered",
    /// The Global fields 13, 14, and 15 produced no millimetre length factor.
    GlobalLengthUnitUnresolved => "global.length-unit-unresolved",
    /// Global framing is recoverable but noncanonical for the declared profile.
    GlobalNoncanonicalFraming => "global.noncanonical-framing",
    /// The declared Global specification version is outside the verified set.
    SourceDialectUnverified => "source.dialect-unverified",
    /// Preserved source image required for a byte-exact write was unavailable.
    PreservedSourceUnavailable => "source.preserved-image-unavailable",
    /// Procedural definitions were reduced to writable solved carriers.
    ProceduralReduced => "geometry.procedural-reduced",
    /// Different coedge pcurves were compared at discrete parameters.
    EdgePcurveAgreementSampled => "geometry.edge-pcurve-agreement-sampled",
    /// A recovered edge was fitted at sampled parameters.
    EdgePcurveSampledToBspline => "geometry.edge-pcurve-sampled-to-bspline",
    /// A native passthrough arena is not regenerated by the semantic writer.
    PassthroughRecordOmitted => "writer.passthrough-omitted",
    /// The emitted Global minimum resolution exceeds the neutral declaration.
    WriterMinimumResolutionAdjusted => "writer.minimum-resolution-adjusted",
    /// An unowned dependent edge was withheld.
    WriterSupportEdgeNotRepresented => "writer.support-edge-not-represented",
    /// Free-geometry points or vertices have no topology output, or an
    /// analytic surface on no face has no bounded output.
    WriterFreeGeometryOmitted => "writer.free-geometry-omitted",
    /// Free edges or curves that duplicate a written carrier were not written.
    WriterDuplicateCarrierOmitted => "writer.duplicate-carrier-omitted",
    /// Unowned support geometry was withheld to avoid standalone transfer.
    WriterSupportGeometryNotRepresented => "writer.support-geometry-not-represented",
    /// A body name cannot be encoded as a Type 406 Form 15 name.
    WriterBodyNameNotRepresented => "writer.body-name-not-represented",
    /// A body has no owning Directory Entry for its color.
    WriterBodyColorNotRepresented => "writer.body-color-not-represented",
    /// IGES Directory color cannot carry a body opacity value.
    WriterBodyOpacityNotRepresented => "writer.body-opacity-not-represented",
    /// A free-geometry body has no single Directory Entry for visibility.
    WriterBodyVisibilityNotRepresented => "writer.body-visibility-not-represented",
    /// A face the writer cannot write was dropped with the records only it uses.
    WriterFaceOmitted => "writer.face-omitted",
    /// A body the writer cannot write was dropped with the records only it uses.
    WriterBodyOmitted => "writer.body-omitted",
    /// An open shell was written as unjoined trimmed surfaces for a version
    /// that defines no open shell.
    WriterOpenShellUnjoined => "writer.open-shell-unjoined",
    /// A solid was written as unjoined trimmed surfaces for a version that
    /// defines no manifold solid B-rep.
    WriterSolidUnjoined => "writer.solid-unjoined",
    /// An open edge with no stated range was written over one of several
    /// ranges its vertices bound, or between its vertices' feet on its curve.
    WriterEdgeRangeAssumed => "writer.edge-range-assumed",
    /// A face was written with its model-space boundary, without the parameter
    /// curves that have no exact image on its written support.
    WriterPcurveOmitted => "writer.pcurve-omitted",
    /// A face whose outer and inner loops have no Type 142 form was written
    /// as a Type 143 with its loops unclassified.
    WriterLoopClassificationOmitted => "writer.loop-classification-omitted",
    /// A vertex tolerance was raised to its distance from an edge's curve.
    VertexToleranceWidened => "geometry.vertex-tolerance-widened",
    /// The decoded document holds an error-severity validation finding.
    ProjectionInvalid => "projection.invalid",
}

impl IgesLossCode {
    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::PreservedSourceUnavailable | Self::GlobalLengthUnitUnresolved => {
                Severity::Blocking
            }
            Self::ProceduralReduced | Self::VertexToleranceWidened => Severity::Info,
            Self::EdgePcurveAgreementSampled | Self::EdgePcurveSampledToBspline => {
                Severity::Warning
            }
            Self::OccurrenceExpansionOutputTruncated
            | Self::OccurrenceExpansionDepthTruncated
            | Self::OccurrenceRootInferenceBlocked
            | Self::OccurrencePlacementMalformed
            | Self::EntityRetainedUnprojected
            | Self::EntityOutsideEnvelope
            | Self::EntityNotProjected
            | Self::GeometryNotProjected
            | Self::ParameterLiteralUnusable
            | Self::SplineClaimRecovered
            | Self::NurbsTransformNonFinite
            | Self::BoundaryPcurveOutsideSupportDomain
            | Self::BoundaryPcurveDisagrees
            | Self::BoundaryWindingUnresolved
            | Self::TopologyFaceBoundaryUnstated
            | Self::PointerUnresolved
            | Self::ParameterBoundaryAmbiguous
            | Self::ParameterCountOverdeclared
            | Self::AttributeTableCountUnstatable
            | Self::DirectoryRecordQuarantined
            | Self::DirectoryRecordRepeated
            | Self::ParameterDataQuarantined
            | Self::CardFramingRecovered
            | Self::DisplayDataNotProjected
            | Self::FreeGeometryBlankStatusNotStated
            | Self::DrawingPropertyAmbiguous
            | Self::BodyNameAmbiguous
            | Self::LineWeightScaleUnavailable
            | Self::RuledDevelopabilityRecovered
            | Self::RuledDevelopabilityNotTransferred
            | Self::SplineHeaderNotTransferred
            | Self::CompositeCarrierDegraded
            | Self::GlobalMetadataFieldUnusable
            | Self::GlobalSemanticContextSubstituted
            | Self::GlobalNumericSyntaxRecovered
            | Self::DirectoryMetadataUnreadable
            | Self::DirectoryMetadataNoncanonical
            | Self::GlobalNoncanonicalFraming
            | Self::SourceDialectUnverified
            | Self::PassthroughRecordOmitted
            | Self::WriterMinimumResolutionAdjusted
            | Self::WriterSupportGeometryNotRepresented
            | Self::WriterSupportEdgeNotRepresented
            | Self::WriterFreeGeometryOmitted
            | Self::WriterDuplicateCarrierOmitted
            | Self::WriterBodyNameNotRepresented
            | Self::WriterBodyColorNotRepresented
            | Self::WriterBodyOpacityNotRepresented
            | Self::WriterBodyVisibilityNotRepresented
            | Self::WriterEdgeRangeAssumed
            | Self::WriterPcurveOmitted
            | Self::WriterLoopClassificationOmitted
            | Self::WriterOpenShellUnjoined
            | Self::WriterSolidUnjoined
            | Self::ProjectionInvalid => Severity::Warning,
            Self::WriterFaceOmitted | Self::WriterBodyOmitted => Severity::Error,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::WriterSupportEdgeNotRepresented
            | Self::WriterFaceOmitted
            | Self::WriterBodyOmitted
            | Self::WriterOpenShellUnjoined
            | Self::WriterSolidUnjoined
            | Self::WriterLoopClassificationOmitted => LossTaxonomy::TopologyNotTransferred,
            Self::WriterEdgeRangeAssumed => LossTaxonomy::TopologyGaugeSubstituted,
            Self::VertexToleranceWidened
            | Self::OccurrenceExpansionOutputTruncated
            | Self::OccurrenceExpansionDepthTruncated
            | Self::OccurrenceRootInferenceBlocked
            | Self::OccurrencePlacementMalformed
            | Self::ProjectionInvalid => LossTaxonomy::DecodeDiagnostic,
            Self::EntityRetainedUnprojected
            | Self::EntityOutsideEnvelope
            | Self::EntityNotProjected => LossTaxonomy::RecordNotTyped,
            Self::BoundaryPcurveOutsideSupportDomain | Self::BoundaryPcurveDisagrees => {
                LossTaxonomy::SourceTopologyInvalid
            }
            Self::BoundaryWindingUnresolved => LossTaxonomy::TopologyGaugeSubstituted,
            Self::TopologyFaceBoundaryUnstated => LossTaxonomy::TopologyNotTransferred,
            Self::PointerUnresolved => LossTaxonomy::ReferenceGraphNotClosed,
            Self::ParameterBoundaryAmbiguous => LossTaxonomy::DecodeDiagnostic,
            Self::DisplayDataNotProjected | Self::LineWeightScaleUnavailable => {
                LossTaxonomy::MaterialNotTransferred
            }
            Self::DrawingPropertyAmbiguous
            | Self::BodyNameAmbiguous
            | Self::FreeGeometryBlankStatusNotStated
            | Self::RuledDevelopabilityNotTransferred
            | Self::SplineHeaderNotTransferred
            | Self::GlobalMetadataFieldUnusable => LossTaxonomy::MetadataNotTransferred,
            Self::CompositeCarrierDegraded
            | Self::GlobalLengthUnitUnresolved
            | Self::NurbsTransformNonFinite
            | Self::GeometryNotProjected
            | Self::WriterSupportGeometryNotRepresented
            | Self::WriterFreeGeometryOmitted
            | Self::WriterDuplicateCarrierOmitted
            | Self::WriterPcurveOmitted => LossTaxonomy::GeometryNotTransferred,
            Self::SplineClaimRecovered
            | Self::RuledDevelopabilityRecovered
            | Self::GlobalSemanticContextSubstituted
            | Self::GlobalNumericSyntaxRecovered
            | Self::DirectoryMetadataUnreadable
            | Self::DirectoryMetadataNoncanonical
            | Self::GlobalNoncanonicalFraming
            | Self::ParameterCountOverdeclared
            | Self::AttributeTableCountUnstatable
            | Self::DirectoryRecordQuarantined
            | Self::DirectoryRecordRepeated
            | Self::ParameterDataQuarantined
            | Self::ParameterLiteralUnusable
            | Self::CardFramingRecovered => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::PreservedSourceUnavailable => LossTaxonomy::PreservedSourceUnavailable,
            Self::ProceduralReduced => LossTaxonomy::ProceduralReduced,
            Self::EdgePcurveAgreementSampled | Self::EdgePcurveSampledToBspline => {
                LossTaxonomy::GeometryAgreementSampled
            }
            Self::PassthroughRecordOmitted => LossTaxonomy::PassthroughRecordOmitted,
            Self::WriterMinimumResolutionAdjusted | Self::WriterBodyNameNotRepresented => {
                LossTaxonomy::MetadataNotTransferred
            }
            Self::WriterBodyColorNotRepresented | Self::WriterBodyOpacityNotRepresented => {
                LossTaxonomy::MaterialNotTransferred
            }
            Self::WriterBodyVisibilityNotRepresented => LossTaxonomy::MetadataNotTransferred,
        }
    }

    /// The strict floor `iges.md` (Decode admission) states: each code that
    /// records a structural defect of the file pins Warning.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::PointerUnresolved
            | Self::CompositeCarrierDegraded
            | Self::SourceDialectUnverified
            | Self::GlobalSemanticContextSubstituted
            | Self::GlobalNumericSyntaxRecovered
            | Self::GlobalLengthUnitUnresolved
            | Self::GlobalNoncanonicalFraming
            | Self::ParameterLiteralUnusable
            | Self::ParameterCountOverdeclared
            | Self::CardFramingRecovered => Some(Severity::Warning),
            _ => None,
        }
    }

    /// Namespaced [`LossKind`] for this local code.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        cadmpeg_ir::report::loss::NamespacedLossKind::new(
            const {
                match cadmpeg_ir::report::loss::LossNamespace::new("iges") {
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
    /// The structured code is `iges/<local>`; the message is the per-instance
    /// text only. Severity and strict floor come from the local code.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::IgesLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = IgesLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "directory.metadata-unreadable",
                "directory.metadata-noncanonical",
                "occurrence.expansion-output-truncated",
                "occurrence.expansion-depth-truncated",
                "occurrence.root-inference-blocked",
                "occurrence.placement-malformed",
                "entity.retained-unprojected",
                "entity.outside-envelope",
                "entity.not-projected",
                "geometry.not-projected",
                "parameter.literal-unusable",
                "geometry.spline-claim-recovered",
                "geometry.nurbs-transform-non-finite",
                "topology.boundary-pcurve-outside-support-domain",
                "topology.boundary-pcurve-disagrees",
                "topology.boundary-winding-unresolved",
                "topology.face-boundary-unstated",
                "graph.pointer-unresolved",
                "parameter.boundary-ambiguous",
                "parameter.count-overdeclared",
                "parameter.attribute-table-count-unstatable",
                "directory.record-quarantined",
                "directory.record-repeated",
                "parameter.data-quarantined",
                "card.framing-recovered",
                "presentation.display-data-not-projected",
                "presentation.free-geometry-blank-status-not-stated",
                "presentation.drawing-property-ambiguous",
                "presentation.body-name-ambiguous",
                "presentation.line-weight-scale-unavailable",
                "geometry.ruled-developability-recovered",
                "geometry.ruled-developability-not-transferred",
                "geometry.spline-header-not-transferred",
                "curve.composite-carrier-degraded",
                "global.metadata-field-unusable",
                "global.semantic-context-substituted",
                "global.numeric-syntax-recovered",
                "global.length-unit-unresolved",
                "global.noncanonical-framing",
                "source.dialect-unverified",
                "source.preserved-image-unavailable",
                "geometry.procedural-reduced",
                "geometry.edge-pcurve-agreement-sampled",
                "geometry.edge-pcurve-sampled-to-bspline",
                "writer.passthrough-omitted",
                "writer.minimum-resolution-adjusted",
                "writer.support-edge-not-represented",
                "writer.free-geometry-omitted",
                "writer.duplicate-carrier-omitted",
                "writer.support-geometry-not-represented",
                "writer.body-name-not-represented",
                "writer.body-color-not-represented",
                "writer.body-opacity-not-represented",
                "writer.body-visibility-not-represented",
                "writer.face-omitted",
                "writer.body-omitted",
                "writer.open-shell-unjoined",
                "writer.solid-unjoined",
                "writer.edge-range-assumed",
                "writer.pcurve-omitted",
                "writer.loop-classification-omitted",
                "geometry.vertex-tolerance-widened",
                "projection.invalid",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in IgesLossCode::ALL {
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
    fn the_unjoined_solid_and_the_omitted_pcurve_pin_no_strict_floor() {
        for code in [
            IgesLossCode::WriterSolidUnjoined,
            IgesLossCode::WriterPcurveOmitted,
        ] {
            assert_eq!(code.note("").code.strict_floor(), None, "{}", code.code());
        }
    }

    /// A code that charges only beside a give-up leaves the refusal to
    /// `record.unreadable`.
    #[test]
    fn the_give_up_codes_pin_no_strict_floor() {
        for code in [
            IgesLossCode::DirectoryRecordQuarantined,
            IgesLossCode::DirectoryRecordRepeated,
            IgesLossCode::ParameterDataQuarantined,
        ] {
            assert_eq!(code.note("").code.strict_floor(), None, "{}", code.code());
        }
    }

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in IgesLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
