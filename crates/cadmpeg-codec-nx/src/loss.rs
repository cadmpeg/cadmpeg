// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for NX `.prt` decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`NxLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`NxLossCode::note`] is the single practical construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `nx` namespace.
//!
//! [`NxLossCode::shared_taxonomy`] is an exhaustive match with no fall-through
//! arm. A default arm would silently assign a category to a code added later,
//! and the categories this codec spans (carrier, topology, history, container)
//! have no honest common default.

use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};

macro_rules! loss_codes {
    ($( $(#[$meta:meta])* $variant:ident => ($code:literal, $severity:ident, $taxonomy:ident), )*) => {
        /// Stable NX transfer-loss identifier.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum NxLossCode {
            $( $(#[$meta])* $variant, )*
        }

        impl NxLossCode {
            /// Every code in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// Stable loss code string.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self { $(Self::$variant => $code),* }
            }

            /// Loss severity.
            #[must_use]
            pub const fn severity(self) -> Severity {
                match self { $(Self::$variant => Severity::$severity),* }
            }

            const fn shared_taxonomy(self) -> LossTaxonomy {
                match self { $(Self::$variant => LossTaxonomy::$taxonomy),* }
            }
        }
    };
}

loss_codes! {
    /// A JT display graph failed native admission.
    DisplayJtGraphRejected => ("container.display-jt-graph-rejected", Blocking, DecodeDiagnostic),
    /// A roll-forward table failed native admission.
    RollForwardTableRejected => ("history.roll-forward-table-rejected", Blocking, DecodeDiagnostic),
    /// An embedded kernel dialect had no declared grammar and was recovered as residual.
    KernelDialectUnverified => ("source.kernel-dialect-unverified", Warning, SourceDialectUnverified),
    /// Two embedded kernel carriers resolved to one dialect-layer identity.
    DialectLayerCollision => ("source.dialect-layer-collision", Warning, DecodeDiagnostic),
    /// Census of decoded Parasolid POINT and analytic curve/surface carriers.
    CarrierAnalyticCensus => ("carrier.analytic-census", Info, CarrierSummary),
    /// Census of decoded embedded JT display tessellations.
    CarrierTessellationCensus => ("carrier.tessellation-census", Info, CarrierSummary),
    /// B-rep topology graph was not reconstructed from surviving typed records.
    TopologyGraphNotReconstructed => ("topology.graph-not-reconstructed", Blocking, TopologyNotTransferred),
    /// Surface-intersection records lack a validated chart and term-endpoint witness.
    IntersectionRecordsOpaque => ("intersection.records-opaque", Warning, ObjectRecordsUntransferred),
    /// Geometric completion reached a declared work bound before all
    /// intersection pcurve lanes were complete.
    IntersectionPcurveCompletionBounded => ("intersection.pcurve-completion-bounded", Warning, ObjectRecordsUntransferred),
    /// Adaptive geometry certification reached its model-wide work bound.
    GeometryAdaptiveWorkBounded => ("geometry.adaptive-work-bounded", Warning, ObjectRecordsUntransferred),
    /// Parasolid deltas applied; every terminal tombstone resolved to a key.
    DeltasApplied => ("deltas.applied", Info, DecodeDiagnostic),
    /// Parasolid deltas applied; one or more terminal tombstones remain unmatched.
    DeltasUnmatchedTombstones => ("deltas.unmatched-tombstones", Warning, DecodeDiagnostic),
    /// Sub-body partitions remain; Boolean history does not resolve every image.
    SubBodyCompositionUnresolved => ("history.sub-body-composition-unresolved", Warning, FeatureHistoryRetained),
    /// A referenced Parasolid attribute value relation did not resolve.
    AttributeValueUnresolved => ("attribute.value-unresolved", Warning, AttributesNotTransferred),
    /// Feature-history suppression state remains unresolved.
    FeatureSuppressionUnresolved => ("feature.suppression-unresolved", Warning, FeatureHistoryRetained),
    /// Configuration activation, body membership, or evaluated state is incomplete.
    ConfigurationStateUnresolved => ("configuration.state-unresolved", Warning, FeatureHistoryRetained),
    /// Expression parameter evaluation or dependency semantics are incomplete.
    ExpressionParameterIncomplete => ("expression.parameter-incomplete", Warning, FeatureHistoryRetained),
    /// Feature-history operations remain native-only without neutral semantics.
    FeatureNativeKindRetained => ("feature.native-kind-retained", Warning, FeatureHistoryRetained),
    /// Feature family identities transferred; construction semantics unresolved.
    FeatureFamilyConstructionUnresolved => ("feature.family-construction-unresolved", Warning, FeatureHistoryRetained),
    /// Typed feature output lineage is missing, duplicated, or unresolved.
    FeatureOutputLineageIncomplete => ("feature.output-lineage-incomplete", Warning, FeatureHistoryRetained),
    /// Typed feature operations have incomplete neutral construction fields.
    FeatureConstructionIncomplete => ("feature.construction-incomplete", Warning, FeatureHistoryRetained),
    /// Sketch history features have no neutral sketch graph.
    SketchGraphUnresolved => ("sketch.graph-unresolved", Warning, FeatureHistoryRetained),
    /// Sketch geometry or constraint records remain native-only.
    SketchNativeSemantics => ("sketch.native-semantics", Warning, FeatureHistoryRetained),
    /// Bounded offset-store control blocks have no admitted complete grammar.
    OffsetStoreControlUntyped => ("container.offset-store-control-untyped", Warning, RecordNotTyped),
    /// A named container stream is retained byte-exact without typed fields.
    ContainerStreamOpaque => ("container.stream-opaque", Info, RecordNotTyped),
    /// A classified non-Parasolid stream was not transferred.
    NonParasolidStreamOmitted => ("stream.non-parasolid-omitted", Info, PassthroughRecordOmitted),
    /// Assembly `.prt` has no inline geometry; children live in external parts.
    AssemblyComponentsExternal => ("assembly.components-external", Blocking, AssemblyComponentsExternal),
    /// No gate-passing analytic carrier was found in the Parasolid streams.
    GeometryNotTransferred => ("geometry.not-transferred", Blocking, GeometryNotTransferred),
}

impl NxLossCode {
    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub fn kind(self) -> LossKind {
        LossKind::namespaced(
            const {
                match cadmpeg_ir::report::LossNamespace::new("nx") {
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
    /// The structured code is `nx/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::NxLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = NxLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "container.display-jt-graph-rejected",
                "history.roll-forward-table-rejected",
                "source.kernel-dialect-unverified",
                "source.dialect-layer-collision",
                "carrier.analytic-census",
                "carrier.tessellation-census",
                "topology.graph-not-reconstructed",
                "intersection.records-opaque",
                "intersection.pcurve-completion-bounded",
                "geometry.adaptive-work-bounded",
                "deltas.applied",
                "deltas.unmatched-tombstones",
                "history.sub-body-composition-unresolved",
                "attribute.value-unresolved",
                "feature.suppression-unresolved",
                "configuration.state-unresolved",
                "expression.parameter-incomplete",
                "feature.native-kind-retained",
                "feature.family-construction-unresolved",
                "feature.output-lineage-incomplete",
                "feature.construction-incomplete",
                "sketch.graph-unresolved",
                "sketch.native-semantics",
                "container.offset-store-control-untyped",
                "container.stream-opaque",
                "stream.non-parasolid-omitted",
                "assembly.components-external",
                "geometry.not-transferred",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in NxLossCode::ALL {
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
        for code in NxLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
