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

use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};
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
            pub(crate) const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The stable string identifier.
            #[must_use]
            pub(crate) const fn code(self) -> &'static str {
                match self { $(Self::$variant => $code),+ }
            }
        }
    };
}

loss_codes! {
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
    /// A NURBS coordinate or parameter transformation produced a non-finite value.
    NurbsTransformNonFinite => "geometry.nurbs-transform-non-finite",
    /// A boundary pcurve leaves the finite parameter domain of its support surface.
    BoundaryPcurveOutsideSupportDomain => "topology.boundary-pcurve-outside-support-domain",
    /// A Directory Entry pointer did not resolve to the expected target.
    PointerUnresolved => "graph.pointer-unresolved",
    /// Parameter Data has more than one structural trailing pointer-group boundary.
    ParameterBoundaryAmbiguous => "parameter.boundary-ambiguous",
    /// A counted list declares more items than its Parameter Data record holds.
    ParameterCountOverdeclared => "parameter.count-overdeclared",
    /// One Directory Entry record kept its raw cards because its typed fields were not recovered.
    DirectoryRecordQuarantined => "directory.record-quarantined",
    /// One entity's Parameter Data kept its raw cards because its tokens were not recovered.
    ParameterDataQuarantined => "parameter.data-quarantined",
    /// Card framing came from the card census because the file's own declaration did not.
    CardFramingRecovered => "card.framing-recovered",
    /// Directory display, font, or color data was not projected.
    DisplayDataNotProjected => "presentation.display-data-not-projected",
    /// A drawing has conflicting valid properties of the same form.
    DrawingPropertyAmbiguous => "presentation.drawing-property-ambiguous",
    /// The Global line-weight scale is unavailable, so no entity has a width.
    LineWeightScaleUnavailable => "presentation.line-weight-scale-unavailable",
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
    /// The selected write target differs from the same-format source dialect.
    SourceDialectDisplaced => "target.source-dialect-displaced",
    /// Preserved source image required for a byte-exact write was unavailable.
    PreservedSourceUnavailable => "source.preserved-image-unavailable",
    /// Procedural definitions were reduced to writable solved carriers.
    ProceduralReduced => "geometry.procedural-reduced",
    /// A native passthrough arena is not regenerated by the semantic writer.
    PassthroughRecordOmitted => "writer.passthrough-omitted",
    /// The emitted Global minimum resolution exceeds the neutral declaration.
    WriterMinimumResolutionAdjusted => "writer.minimum-resolution-adjusted",
}

impl IgesLossCode {
    /// The severity of this loss.
    #[must_use]
    pub(crate) const fn severity(self) -> Severity {
        match self {
            Self::PreservedSourceUnavailable | Self::GlobalLengthUnitUnresolved => {
                Severity::Blocking
            }
            Self::ProceduralReduced => Severity::Info,
            Self::OccurrenceExpansionOutputTruncated
            | Self::OccurrenceExpansionDepthTruncated
            | Self::OccurrenceRootInferenceBlocked
            | Self::OccurrencePlacementMalformed
            | Self::EntityRetainedUnprojected
            | Self::EntityOutsideEnvelope
            | Self::EntityNotProjected
            | Self::NurbsTransformNonFinite
            | Self::BoundaryPcurveOutsideSupportDomain
            | Self::PointerUnresolved
            | Self::ParameterBoundaryAmbiguous
            | Self::ParameterCountOverdeclared
            | Self::DirectoryRecordQuarantined
            | Self::ParameterDataQuarantined
            | Self::CardFramingRecovered
            | Self::DisplayDataNotProjected
            | Self::DrawingPropertyAmbiguous
            | Self::LineWeightScaleUnavailable
            | Self::RuledDevelopabilityNotTransferred
            | Self::SplineHeaderNotTransferred
            | Self::CompositeCarrierDegraded
            | Self::GlobalMetadataFieldUnusable
            | Self::GlobalSemanticContextSubstituted
            | Self::GlobalNumericSyntaxRecovered
            | Self::GlobalNoncanonicalFraming
            | Self::SourceDialectUnverified
            | Self::SourceDialectDisplaced
            | Self::PassthroughRecordOmitted
            | Self::WriterMinimumResolutionAdjusted => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::OccurrenceExpansionOutputTruncated
            | Self::OccurrenceExpansionDepthTruncated
            | Self::OccurrenceRootInferenceBlocked
            | Self::OccurrencePlacementMalformed => LossTaxonomy::DecodeDiagnostic,
            Self::EntityRetainedUnprojected
            | Self::EntityOutsideEnvelope
            | Self::EntityNotProjected => LossTaxonomy::RecordNotTyped,
            Self::BoundaryPcurveOutsideSupportDomain => LossTaxonomy::SourceTopologyInvalid,
            Self::PointerUnresolved => LossTaxonomy::ReferenceGraphNotClosed,
            Self::ParameterBoundaryAmbiguous => LossTaxonomy::DecodeDiagnostic,
            Self::DisplayDataNotProjected | Self::LineWeightScaleUnavailable => {
                LossTaxonomy::MaterialNotTransferred
            }
            Self::DrawingPropertyAmbiguous
            | Self::RuledDevelopabilityNotTransferred
            | Self::SplineHeaderNotTransferred
            | Self::GlobalMetadataFieldUnusable => LossTaxonomy::MetadataNotTransferred,
            Self::CompositeCarrierDegraded
            | Self::GlobalLengthUnitUnresolved
            | Self::NurbsTransformNonFinite => LossTaxonomy::GeometryNotTransferred,
            Self::GlobalSemanticContextSubstituted
            | Self::GlobalNumericSyntaxRecovered
            | Self::GlobalNoncanonicalFraming
            | Self::ParameterCountOverdeclared
            | Self::DirectoryRecordQuarantined
            | Self::ParameterDataQuarantined
            | Self::CardFramingRecovered => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::SourceDialectDisplaced => LossTaxonomy::SourceDialectDisplaced,
            Self::PreservedSourceUnavailable => LossTaxonomy::PreservedSourceUnavailable,
            Self::ProceduralReduced => LossTaxonomy::ProceduralReduced,
            Self::PassthroughRecordOmitted => LossTaxonomy::PassthroughRecordOmitted,
            Self::WriterMinimumResolutionAdjusted => LossTaxonomy::MetadataNotTransferred,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(
            const {
                match cadmpeg_ir::report::LossNamespace::new("iges") {
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
    /// The structured code is `iges/<local>`; the message is the per-instance
    /// text only. Severity comes from the local code and the strict floor from
    /// the shared taxonomy.
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
                "occurrence.expansion-output-truncated",
                "occurrence.expansion-depth-truncated",
                "occurrence.root-inference-blocked",
                "occurrence.placement-malformed",
                "entity.retained-unprojected",
                "entity.outside-envelope",
                "entity.not-projected",
                "geometry.nurbs-transform-non-finite",
                "topology.boundary-pcurve-outside-support-domain",
                "graph.pointer-unresolved",
                "parameter.boundary-ambiguous",
                "parameter.count-overdeclared",
                "directory.record-quarantined",
                "parameter.data-quarantined",
                "card.framing-recovered",
                "presentation.display-data-not-projected",
                "presentation.drawing-property-ambiguous",
                "presentation.line-weight-scale-unavailable",
                "geometry.ruled-developability-not-transferred",
                "geometry.spline-header-not-transferred",
                "curve.composite-carrier-degraded",
                "global.metadata-field-unusable",
                "global.semantic-context-substituted",
                "global.numeric-syntax-recovered",
                "global.length-unit-unresolved",
                "global.noncanonical-framing",
                "source.dialect-unverified",
                "target.source-dialect-displaced",
                "source.preserved-image-unavailable",
                "geometry.procedural-reduced",
                "writer.passthrough-omitted",
                "writer.minimum-resolution-adjusted",
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
