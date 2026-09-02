// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for CATIA V5 `.CATPart` decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`CatiaLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`CatiaLossCode::note`] is the single practical construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller. Local codes appear on
//! [`LossNote::code`] under the `catia` namespace.
//!
//! [`CatiaLossCode::shared_taxonomy`] is an exhaustive match with no fall-through
//! arm. A default arm would silently assign a category to a code added later,
//! and the categories this codec spans (geometry, topology, history, attribute,
//! container) have no honest common default.
//!
use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};

/// A stable, machine-readable identifier for one CATIA V5 transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`CatiaLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CatiaLossCode {
    /// The storage layout matched no declared dialect's structural invariants.
    SourceDialectUnverified,
    /// Verbatim vertex points and analytic surface carriers were decoded.
    GeometryCarrierSummary,
    /// Transferred model retains unresolved curve or surface carriers.
    GeometryUnresolvedCarriers,
    /// No B-rep geometry was transferred for this storage variant.
    GeometryBrepNotTransferred,
    /// Plane surface records lacked valid tag-bridged parameter records.
    GeometryPlaneParametersInvalid,
    /// Analytic surface records had a non-finite or out-of-range payload.
    GeometryAnalyticPayloadInvalid,
    /// Face-local free-form carriers retain identity without aliased geometry.
    GeometryFaceLocalFreeformNotTransferred,
    /// Revolution carriers retain profile identity without bound directrices.
    GeometryRevolutionProfileUnbound,
    /// Consolidated line-profile records were not transferred by the active route.
    GeometryLineProfileNotTransferred,
    /// Detected FBB face rows were not emitted as a B-rep boundary graph.
    TopologyBoundaryGraphNotEmitted,
    /// Candidate FBB face rows were withheld from the standard topology population.
    TopologyFbbRowsWithheld,
    /// B-rep topology graph was not built for this file.
    TopologyGraphNotBuilt,
    /// E5 reference graph is closed; orientation uses an incidence-derived gauge.
    TopologyE5GaugeSubstituted,
    /// E5 carriers decoded, but the reference graph did not close.
    TopologyE5GraphUnclosed,
    /// B5 reference graph is closed; face sense and body kind use a topology gauge.
    TopologyB5GaugeSubstituted,
    /// A maximal reference-closed B5 subset transferred; remaining nodes stay native.
    TopologyB5SubsetIncomplete,
    /// Object-stream graph exceeded the bounded work slice.
    TopologyObjectStreamWorkSliceExhausted,
    /// Object-stream and NURBS carriers decoded, but the B5 graph did not close.
    TopologyB5GraphUnclosed,
    /// Zero-entity support runs retain face-local occurrences without closed topology.
    TopologyZeroEntitySupportsRetained,
    /// Zero-entity topology transferred under a derived or source-bound gauge.
    TopologyZeroEntityGaugeSubstituted,
    /// Zero-entity loop members bind supports, but no neutral topology transferred.
    TopologyZeroEntityNotTransferred,
    /// Zero-entity wire loops transferred; face topology remains unresolved.
    TopologyZeroEntityFaceUnresolved,
    /// Outer declarations do not select one physically contained object graph.
    HistoryModelingScopeUnresolved,
    /// Modeling-scope object-graph field records remain unresolved.
    HistoryObjectRecordsUnresolved,
    /// Legacy design runs retain unresolved selector, relation, and feature semantics.
    HistoryLegacyRunsUnresolved,
    /// Materials, unbound NURBS caches, and document metadata were not transferred.
    AttributesMaterialsMetadataNotTransferred,
    /// Dimension scalars retain no admitted physical-quantity discriminator.
    AttributesDimensionQuantityUnresolved,
    /// Visualization value blocks lack a proven typed face or body target.
    AttributesVisualizationUnbound,
}

impl CatiaLossCode {
    /// Every code, in declaration order.
    pub const ALL: &'static [CatiaLossCode] = &[
        Self::SourceDialectUnverified,
        Self::GeometryCarrierSummary,
        Self::GeometryUnresolvedCarriers,
        Self::GeometryBrepNotTransferred,
        Self::GeometryPlaneParametersInvalid,
        Self::GeometryAnalyticPayloadInvalid,
        Self::GeometryFaceLocalFreeformNotTransferred,
        Self::GeometryRevolutionProfileUnbound,
        Self::GeometryLineProfileNotTransferred,
        Self::TopologyBoundaryGraphNotEmitted,
        Self::TopologyFbbRowsWithheld,
        Self::TopologyGraphNotBuilt,
        Self::TopologyE5GaugeSubstituted,
        Self::TopologyE5GraphUnclosed,
        Self::TopologyB5GaugeSubstituted,
        Self::TopologyB5SubsetIncomplete,
        Self::TopologyObjectStreamWorkSliceExhausted,
        Self::TopologyB5GraphUnclosed,
        Self::TopologyZeroEntitySupportsRetained,
        Self::TopologyZeroEntityGaugeSubstituted,
        Self::TopologyZeroEntityNotTransferred,
        Self::TopologyZeroEntityFaceUnresolved,
        Self::HistoryModelingScopeUnresolved,
        Self::HistoryObjectRecordsUnresolved,
        Self::HistoryLegacyRunsUnresolved,
        Self::AttributesMaterialsMetadataNotTransferred,
        Self::AttributesDimensionQuantityUnresolved,
        Self::AttributesVisualizationUnbound,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::GeometryCarrierSummary => "geometry.carrier-summary",
            Self::GeometryUnresolvedCarriers => "geometry.unresolved-carriers",
            Self::GeometryBrepNotTransferred => "geometry.brep-not-transferred",
            Self::GeometryPlaneParametersInvalid => "geometry.plane-parameters-invalid",
            Self::GeometryAnalyticPayloadInvalid => "geometry.analytic-payload-invalid",
            Self::GeometryFaceLocalFreeformNotTransferred => {
                "geometry.face-local-freeform-not-transferred"
            }
            Self::GeometryRevolutionProfileUnbound => "geometry.revolution-profile-unbound",
            Self::GeometryLineProfileNotTransferred => "geometry.line-profile-not-transferred",
            Self::TopologyBoundaryGraphNotEmitted => "topology.boundary-graph-not-emitted",
            Self::TopologyFbbRowsWithheld => "topology.fbb-rows-withheld",
            Self::TopologyGraphNotBuilt => "topology.graph-not-built",
            Self::TopologyE5GaugeSubstituted => "topology.e5-gauge-substituted",
            Self::TopologyE5GraphUnclosed => "topology.e5-graph-unclosed",
            Self::TopologyB5GaugeSubstituted => "topology.b5-gauge-substituted",
            Self::TopologyB5SubsetIncomplete => "topology.b5-subset-incomplete",
            Self::TopologyObjectStreamWorkSliceExhausted => {
                "topology.object-stream-work-slice-exhausted"
            }
            Self::TopologyB5GraphUnclosed => "topology.b5-graph-unclosed",
            Self::TopologyZeroEntitySupportsRetained => "topology.zero-entity-supports-retained",
            Self::TopologyZeroEntityGaugeSubstituted => "topology.zero-entity-gauge-substituted",
            Self::TopologyZeroEntityNotTransferred => "topology.zero-entity-not-transferred",
            Self::TopologyZeroEntityFaceUnresolved => "topology.zero-entity-face-unresolved",
            Self::HistoryModelingScopeUnresolved => "history.modeling-scope-unresolved",
            Self::HistoryObjectRecordsUnresolved => "history.object-records-unresolved",
            Self::HistoryLegacyRunsUnresolved => "history.legacy-runs-unresolved",
            Self::AttributesMaterialsMetadataNotTransferred => {
                "attributes.materials-metadata-not-transferred"
            }
            Self::AttributesDimensionQuantityUnresolved => {
                "attributes.dimension-quantity-unresolved"
            }
            Self::AttributesVisualizationUnbound => "attributes.visualization-unbound",
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::GeometryCarrierSummary => Severity::Info,
            Self::GeometryUnresolvedCarriers
            | Self::GeometryBrepNotTransferred
            | Self::TopologyBoundaryGraphNotEmitted
            | Self::TopologyFbbRowsWithheld
            | Self::TopologyGraphNotBuilt
            | Self::TopologyE5GraphUnclosed
            | Self::TopologyB5SubsetIncomplete
            | Self::TopologyObjectStreamWorkSliceExhausted
            | Self::TopologyB5GraphUnclosed
            | Self::TopologyZeroEntityNotTransferred
            | Self::TopologyZeroEntityFaceUnresolved
            | Self::HistoryModelingScopeUnresolved
            | Self::HistoryObjectRecordsUnresolved
            | Self::HistoryLegacyRunsUnresolved => Severity::Blocking,
            _ => Severity::Warning,
        }
    }

    /// The shared cross-codec category this loss reports under.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::GeometryCarrierSummary => LossTaxonomy::CarrierSummary,
            Self::GeometryUnresolvedCarriers
            | Self::GeometryBrepNotTransferred
            | Self::GeometryPlaneParametersInvalid
            | Self::GeometryAnalyticPayloadInvalid
            | Self::GeometryFaceLocalFreeformNotTransferred
            | Self::GeometryRevolutionProfileUnbound
            | Self::GeometryLineProfileNotTransferred => LossTaxonomy::GeometryNotTransferred,
            Self::TopologyBoundaryGraphNotEmitted
            | Self::TopologyFbbRowsWithheld
            | Self::TopologyGraphNotBuilt
            | Self::TopologyE5GaugeSubstituted
            | Self::TopologyE5GraphUnclosed
            | Self::TopologyB5GaugeSubstituted
            | Self::TopologyB5SubsetIncomplete
            | Self::TopologyObjectStreamWorkSliceExhausted
            | Self::TopologyB5GraphUnclosed
            | Self::TopologyZeroEntitySupportsRetained
            | Self::TopologyZeroEntityNotTransferred
            | Self::TopologyZeroEntityFaceUnresolved => LossTaxonomy::TopologyNotTransferred,
            Self::TopologyZeroEntityGaugeSubstituted => LossTaxonomy::TopologyGaugeSubstituted,
            Self::AttributesMaterialsMetadataNotTransferred
            | Self::AttributesDimensionQuantityUnresolved
            | Self::AttributesVisualizationUnbound => LossTaxonomy::AttributesNotTransferred,
            Self::HistoryModelingScopeUnresolved
            | Self::HistoryObjectRecordsUnresolved
            | Self::HistoryLegacyRunsUnresolved => LossTaxonomy::FeatureHistoryRetained,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub fn kind(self) -> LossKind {
        LossKind::namespaced("catia", self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `catia/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::CatiaLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = CatiaLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "source.dialect-unverified",
                "geometry.carrier-summary",
                "geometry.unresolved-carriers",
                "geometry.brep-not-transferred",
                "geometry.plane-parameters-invalid",
                "geometry.analytic-payload-invalid",
                "geometry.face-local-freeform-not-transferred",
                "geometry.revolution-profile-unbound",
                "geometry.line-profile-not-transferred",
                "topology.boundary-graph-not-emitted",
                "topology.fbb-rows-withheld",
                "topology.graph-not-built",
                "topology.e5-gauge-substituted",
                "topology.e5-graph-unclosed",
                "topology.b5-gauge-substituted",
                "topology.b5-subset-incomplete",
                "topology.object-stream-work-slice-exhausted",
                "topology.b5-graph-unclosed",
                "topology.zero-entity-supports-retained",
                "topology.zero-entity-gauge-substituted",
                "topology.zero-entity-not-transferred",
                "topology.zero-entity-face-unresolved",
                "history.modeling-scope-unresolved",
                "history.object-records-unresolved",
                "history.legacy-runs-unresolved",
                "attributes.materials-metadata-not-transferred",
                "attributes.dimension-quantity-unresolved",
                "attributes.visualization-unbound",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in CatiaLossCode::ALL {
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
        for code in CatiaLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
