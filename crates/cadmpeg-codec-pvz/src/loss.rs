// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.pvz` package decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`PvzLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text.
//!
//! [`PvzLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate: it fixes the loss category and severity from
//! the code, and it leaves only the per-instance message to the caller.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};
use cadmpeg_ir::report::Severity;

use crate::brep::resolve::ENDPOINT_TOLERANCE;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("pvz");

/// A stable, machine-readable identifier for one `.pvz` transfer loss.
///
/// The string form (via [`PvzLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PvzLossCode {
    /// The structure member carries no signature of a declared dialect row.
    SourceDialectUnverified,
    /// The ZIP central directory was unreadable and members were recovered
    /// from local headers.
    ContainerDirectoryRecovered,
    /// The structure member did not decode: the package has no product
    /// structure, and no geometry member is placed.
    StructureMemberMalformed,
    /// A structure-member stream of an undefined type was not decoded.
    StructureStreamUndecoded,
    /// A property stream did not decode; none of its properties transferred.
    StructurePropertyStreamMalformed,
    /// Structure elements outside the decoded model were not transferred.
    StructureElementUndecoded,
    /// Bytes after a typed definition's name (PVZ-04) were not transferred.
    StructureDefinitionAttributes,
    /// Placement flags and occurrence extension bytes (PVZ-05) were not
    /// transferred.
    StructureOccurrenceAttributes,
    /// An occurrence with an undefined placement encoding was not placed.
    StructureOccurrenceUndecoded,
    /// Geometry-element bounds were not transferred.
    StructureBoundsNotTransferred,
    /// Bytes after a typed property's value (PVZ-09) were not transferred.
    StructurePropertyAttributes,
    /// A property with a blank or repeated name was not transferred.
    StructurePropertyKeyDropped,
    /// The definitions do not have exactly one root.
    StructureRootAmbiguous,
    /// A child occurrence closes a definition cycle and was not expanded.
    StructureOccurrenceCycle,
    /// Occurrence expansion stopped at its count or depth limit.
    StructureOccurrenceTruncated,
    /// A placement basis is not a uniformly scaled rotation; the placement
    /// was kept as a general affine transform.
    StructurePlacementNotSimilarity,
    /// A placement with non-finite values was replaced by the identity.
    StructurePlacementInvalid,
    /// A geometry element names a member that the package does not hold.
    GeometryMemberMissing,
    /// A geometry member's container did not decode; none of its streams
    /// transferred.
    GeometryMemberMalformed,
    /// A geometry member that no definition names was not transferred.
    GeometryMemberUnreferenced,
    /// A geometry-member stream was not decoded.
    GeometryStreamUndecoded,
    /// The OL preamble block (PVZ-11) was not transferred.
    GeometryPreambleUndecoded,
    /// An exact-geometry stream was located but its B-rep was not decoded.
    GeometryExactUndecoded,
    /// Attribute-stream records outside the appearance records (PVZ-13) were
    /// not transferred.
    AttributeRecordsUndecoded,
    /// An appearance definition or reference did not resolve.
    AttributeAppearanceUnresolved,
    /// Appearance-reference tails (PVZ-12) were not transferred.
    AttributeAppearanceTail,
    /// An appearance colour or shading value outside its range was not
    /// transferred.
    AttributeAppearanceColorRejected,
    /// Patch-header face references, flags, and references (PVZ-14) were not
    /// transferred.
    TessellationPatchFields,
    /// Surface-tessellation records outside the patch, vertex, and triangle
    /// records (PVZ-15) were not transferred.
    TessellationRecordsUndecoded,
    /// Vertex texture coordinates (PVZ-17) were not transferred.
    TessellationTextureCoordinates,
    /// Decoded edge tessellation was not transferred.
    TessellationEdgesNotTransferred,
    /// A decoded mesh failed tessellation admission and was not transferred.
    TessellationMeshRejected,
    /// Exact-stream records with a frame and no settled meaning were not
    /// transferred.
    ExactRecordUninterpreted,
    /// A face carrier has no neutral surface form.
    ExactSurfaceUntyped,
    /// An analytic face carrier was written as its embedded B-spline.
    ExactSurfaceAsBspline,
    /// An edge curve has no neutral curve form; the edge has no carrier.
    ExactCurveUntyped,
    /// A coedge pcurve has no neutral pcurve form or domain.
    ExactPcurveUntransferred,
    /// Secondary pcurves of analytic faces lie in the embedded B-spline
    /// domain and were not written onto the analytic carrier.
    ExactSecondaryPcurveOmitted,
    /// Coedge pcurve images miss their edge-curve ends by more than the
    /// endpoint tolerance; the face tolerance admits the measured residual.
    ExactEndpointMismatch,
    /// A loop does not close, or a loop junction joins no vertex.
    ExactLoopOpen,
    /// A face side came from a gauge, not from an outward test.
    ExactFaceSideGauge,
    /// A derived search ran out of work: a face kept one unclassified region,
    /// or coincident edge records stayed unpaired.
    ExactTopologyWorkExhausted,
    /// Two edge records that one model edge stores once per use were written
    /// as one neutral edge on the curve of the first record.
    ExactEdgeUsesMerged,
    /// A face record with no loop bounds no region and was not transferred.
    ExactFaceUnboundedOmitted,
    /// A coedge on a collapsed singular edge away from a chart singularity was
    /// left out of its loop.
    ExactCoedgeDegenerateOmitted,
}

impl PvzLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [PvzLossCode] = &[
        Self::SourceDialectUnverified,
        Self::ContainerDirectoryRecovered,
        Self::StructureMemberMalformed,
        Self::StructureStreamUndecoded,
        Self::StructurePropertyStreamMalformed,
        Self::StructureElementUndecoded,
        Self::StructureDefinitionAttributes,
        Self::StructureOccurrenceAttributes,
        Self::StructureOccurrenceUndecoded,
        Self::StructureBoundsNotTransferred,
        Self::StructurePropertyAttributes,
        Self::StructurePropertyKeyDropped,
        Self::StructureRootAmbiguous,
        Self::StructureOccurrenceCycle,
        Self::StructureOccurrenceTruncated,
        Self::StructurePlacementNotSimilarity,
        Self::StructurePlacementInvalid,
        Self::GeometryMemberMissing,
        Self::GeometryMemberMalformed,
        Self::GeometryMemberUnreferenced,
        Self::GeometryStreamUndecoded,
        Self::GeometryPreambleUndecoded,
        Self::GeometryExactUndecoded,
        Self::AttributeRecordsUndecoded,
        Self::AttributeAppearanceUnresolved,
        Self::AttributeAppearanceTail,
        Self::AttributeAppearanceColorRejected,
        Self::TessellationPatchFields,
        Self::TessellationRecordsUndecoded,
        Self::TessellationTextureCoordinates,
        Self::TessellationEdgesNotTransferred,
        Self::TessellationMeshRejected,
        Self::ExactRecordUninterpreted,
        Self::ExactSurfaceUntyped,
        Self::ExactSurfaceAsBspline,
        Self::ExactCurveUntyped,
        Self::ExactPcurveUntransferred,
        Self::ExactSecondaryPcurveOmitted,
        Self::ExactEndpointMismatch,
        Self::ExactLoopOpen,
        Self::ExactFaceSideGauge,
        Self::ExactTopologyWorkExhausted,
        Self::ExactEdgeUsesMerged,
        Self::ExactFaceUnboundedOmitted,
        Self::ExactCoedgeDegenerateOmitted,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::ContainerDirectoryRecovered => "container.directory-recovered",
            Self::StructureMemberMalformed => "structure.member-malformed",
            Self::StructureStreamUndecoded => "structure.stream-undecoded",
            Self::StructurePropertyStreamMalformed => "structure.property-stream-malformed",
            Self::StructureElementUndecoded => "structure.element-undecoded",
            Self::StructureDefinitionAttributes => "structure.definition-attributes-undecoded",
            Self::StructureOccurrenceAttributes => "structure.occurrence-attributes-undecoded",
            Self::StructureOccurrenceUndecoded => "structure.occurrence-undecoded",
            Self::StructureBoundsNotTransferred => "structure.bounds-not-transferred",
            Self::StructurePropertyAttributes => "structure.property-attributes-undecoded",
            Self::StructurePropertyKeyDropped => "structure.property-key-dropped",
            Self::StructureRootAmbiguous => "structure.root-ambiguous",
            Self::StructureOccurrenceCycle => "structure.occurrence-cycle",
            Self::StructureOccurrenceTruncated => "structure.occurrence-expansion-truncated",
            Self::StructurePlacementNotSimilarity => "structure.placement-not-similarity",
            Self::StructurePlacementInvalid => "structure.placement-invalid",
            Self::GeometryMemberMissing => "geometry.member-missing",
            Self::GeometryMemberMalformed => "geometry.member-malformed",
            Self::GeometryMemberUnreferenced => "geometry.member-unreferenced",
            Self::GeometryStreamUndecoded => "geometry.stream-undecoded",
            Self::GeometryPreambleUndecoded => "geometry.preamble-undecoded",
            Self::GeometryExactUndecoded => "geometry.exact-undecoded",
            Self::AttributeRecordsUndecoded => "attributes.records-undecoded",
            Self::AttributeAppearanceUnresolved => "attributes.appearance-unresolved",
            Self::AttributeAppearanceTail => "attributes.appearance-tail-undecoded",
            Self::AttributeAppearanceColorRejected => "attributes.appearance-color-rejected",
            Self::TessellationPatchFields => "tessellation.patch-fields-undecoded",
            Self::TessellationRecordsUndecoded => "tessellation.records-undecoded",
            Self::TessellationTextureCoordinates => "tessellation.texture-coordinates-dropped",
            Self::TessellationEdgesNotTransferred => "tessellation.edges-not-transferred",
            Self::TessellationMeshRejected => "tessellation.mesh-rejected",
            Self::ExactRecordUninterpreted => "exact.record-uninterpreted",
            Self::ExactSurfaceUntyped => "exact.surface-untyped",
            Self::ExactSurfaceAsBspline => "exact.surface-as-bspline",
            Self::ExactCurveUntyped => "exact.curve-untyped",
            Self::ExactPcurveUntransferred => "exact.pcurve-untransferred",
            Self::ExactSecondaryPcurveOmitted => "exact.secondary-pcurve-omitted",
            Self::ExactEndpointMismatch => "exact.endpoint-mismatch",
            Self::ExactLoopOpen => "exact.loop-open",
            Self::ExactFaceSideGauge => "exact.face-side-gauge",
            Self::ExactTopologyWorkExhausted => "exact.topology-work-exhausted",
            Self::ExactEdgeUsesMerged => "exact.edge-uses-merged",
            Self::ExactFaceUnboundedOmitted => "exact.face-unbounded-omitted",
            Self::ExactCoedgeDegenerateOmitted => "exact.coedge-degenerate-omitted",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::StructureElementUndecoded
            | Self::StructureDefinitionAttributes
            | Self::StructureOccurrenceAttributes
            | Self::StructureBoundsNotTransferred
            | Self::StructurePropertyAttributes
            | Self::GeometryPreambleUndecoded
            | Self::AttributeRecordsUndecoded
            | Self::AttributeAppearanceTail
            | Self::TessellationPatchFields
            | Self::TessellationRecordsUndecoded => Severity::Info,
            Self::SourceDialectUnverified
            | Self::ContainerDirectoryRecovered
            | Self::StructureStreamUndecoded
            | Self::StructurePropertyStreamMalformed
            | Self::StructureOccurrenceUndecoded
            | Self::StructurePropertyKeyDropped
            | Self::StructureRootAmbiguous
            | Self::StructureOccurrenceCycle
            | Self::StructureOccurrenceTruncated
            | Self::StructurePlacementNotSimilarity
            | Self::StructurePlacementInvalid
            | Self::GeometryMemberMissing
            | Self::GeometryMemberMalformed
            | Self::GeometryMemberUnreferenced
            | Self::GeometryStreamUndecoded
            | Self::GeometryExactUndecoded
            | Self::AttributeAppearanceUnresolved
            | Self::AttributeAppearanceColorRejected
            | Self::TessellationTextureCoordinates
            | Self::TessellationEdgesNotTransferred
            | Self::TessellationMeshRejected
            | Self::ExactRecordUninterpreted
            | Self::ExactSurfaceAsBspline
            | Self::ExactCurveUntyped
            | Self::ExactPcurveUntransferred
            | Self::ExactSecondaryPcurveOmitted
            | Self::ExactEndpointMismatch
            | Self::ExactLoopOpen
            | Self::ExactFaceSideGauge
            | Self::ExactTopologyWorkExhausted
            | Self::ExactEdgeUsesMerged
            | Self::ExactFaceUnboundedOmitted
            | Self::ExactCoedgeDegenerateOmitted => Severity::Warning,
            Self::ExactSurfaceUntyped => Severity::Error,
            Self::StructureMemberMalformed => Severity::Blocking,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::ContainerDirectoryRecovered => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::StructureMemberMalformed
            | Self::GeometryMemberMalformed
            | Self::GeometryStreamUndecoded
            | Self::GeometryExactUndecoded
            | Self::TessellationEdgesNotTransferred
            | Self::TessellationMeshRejected => LossTaxonomy::GeometryNotTransferred,
            Self::StructureStreamUndecoded
            | Self::StructureElementUndecoded
            | Self::StructureDefinitionAttributes
            | Self::StructureOccurrenceAttributes
            | Self::StructurePropertyAttributes
            | Self::GeometryPreambleUndecoded
            | Self::TessellationPatchFields
            | Self::TessellationRecordsUndecoded => LossTaxonomy::RecordNotTyped,
            Self::StructurePropertyStreamMalformed
            | Self::StructureBoundsNotTransferred
            | Self::StructurePropertyKeyDropped => LossTaxonomy::MetadataNotTransferred,
            Self::StructureOccurrenceUndecoded
            | Self::StructureRootAmbiguous
            | Self::StructureOccurrenceCycle
            | Self::StructureOccurrenceTruncated
            | Self::StructurePlacementInvalid => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::StructurePlacementNotSimilarity => LossTaxonomy::DecodeDiagnostic,
            Self::GeometryMemberMissing | Self::GeometryMemberUnreferenced => {
                LossTaxonomy::AssetNotTransferred
            }
            Self::AttributeRecordsUndecoded | Self::TessellationTextureCoordinates => {
                LossTaxonomy::AttributesNotTransferred
            }
            Self::AttributeAppearanceUnresolved | Self::AttributeAppearanceTail => {
                LossTaxonomy::MaterialNotTransferred
            }
            Self::AttributeAppearanceColorRejected => LossTaxonomy::AppearanceReduced,
            Self::ExactRecordUninterpreted => LossTaxonomy::RecordNotTyped,
            Self::ExactSurfaceUntyped | Self::ExactCurveUntyped => {
                LossTaxonomy::GeometryNotTransferred
            }
            Self::ExactSurfaceAsBspline | Self::ExactEndpointMismatch => {
                LossTaxonomy::DecodeDiagnostic
            }
            Self::ExactPcurveUntransferred | Self::ExactSecondaryPcurveOmitted => {
                LossTaxonomy::PcurveOmitted
            }
            Self::ExactLoopOpen => LossTaxonomy::SourceTopologyInvalid,
            Self::ExactFaceSideGauge => LossTaxonomy::TopologyGaugeSubstituted,
            Self::ExactTopologyWorkExhausted => LossTaxonomy::TopologyNotTransferred,
            Self::ExactEdgeUsesMerged => LossTaxonomy::DecodeDiagnostic,
            Self::ExactFaceUnboundedOmitted => LossTaxonomy::GeometryNotTransferred,
            Self::ExactCoedgeDegenerateOmitted => LossTaxonomy::TopologyNotTransferred,
        }
    }

    /// The message of a note that counts `count` instances of this code in
    /// the exact-geometry stream of geometry member `member`.
    pub(crate) fn tally_message(self, member: &str, count: usize) -> String {
        let what = match self {
            Self::ExactRecordUninterpreted => {
                "exact-stream records with no settled meaning not transferred (tag 0x0a and \
                 undefined tags)"
                    .to_owned()
            }
            Self::ExactSurfaceUntyped => {
                "faces whose carrier has no neutral surface form".to_owned()
            }
            Self::ExactSurfaceAsBspline => {
                "analytic faces written as their embedded B-spline carrier".to_owned()
            }
            Self::ExactCurveUntyped => {
                "edges without a neutral curve (composite or undefined curve types)".to_owned()
            }
            Self::ExactPcurveUntransferred => "coedge pcurves not transferred".to_owned(),
            Self::ExactSecondaryPcurveOmitted => {
                "secondary pcurves in the embedded B-spline domain not written onto analytic \
                 faces"
                    .to_owned()
            }
            Self::ExactEndpointMismatch => format!(
                "faces whose pcurve images miss the edge ends by more than {ENDPOINT_TOLERANCE:e} m"
            ),
            Self::ExactLoopOpen => "open loops or loop junctions that join no vertex".to_owned(),
            Self::ExactFaceSideGauge => {
                "faces whose side is a propagation gauge, or side conflicts between faces"
                    .to_owned()
            }
            Self::ExactTopologyWorkExhausted => {
                "derived topology searches stopped by the work limit".to_owned()
            }
            Self::ExactEdgeUsesMerged => {
                "edge records written as the edge of a coincident record".to_owned()
            }
            Self::ExactFaceUnboundedOmitted => {
                "face records without a loop not transferred".to_owned()
            }
            Self::ExactCoedgeDegenerateOmitted => {
                "coedges on collapsed singular edges left out of their loops".to_owned()
            }
            other => format!("instances of {}", other.code()),
        };
        format!("geometry member {member:?}: {count} {what}")
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(NAMESPACE, self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `pvz/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::PvzLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = PvzLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "source.dialect-unverified",
                "container.directory-recovered",
                "structure.member-malformed",
                "structure.stream-undecoded",
                "structure.property-stream-malformed",
                "structure.element-undecoded",
                "structure.definition-attributes-undecoded",
                "structure.occurrence-attributes-undecoded",
                "structure.occurrence-undecoded",
                "structure.bounds-not-transferred",
                "structure.property-attributes-undecoded",
                "structure.property-key-dropped",
                "structure.root-ambiguous",
                "structure.occurrence-cycle",
                "structure.occurrence-expansion-truncated",
                "structure.placement-not-similarity",
                "structure.placement-invalid",
                "geometry.member-missing",
                "geometry.member-malformed",
                "geometry.member-unreferenced",
                "geometry.stream-undecoded",
                "geometry.preamble-undecoded",
                "geometry.exact-undecoded",
                "attributes.records-undecoded",
                "attributes.appearance-unresolved",
                "attributes.appearance-tail-undecoded",
                "attributes.appearance-color-rejected",
                "tessellation.patch-fields-undecoded",
                "tessellation.records-undecoded",
                "tessellation.texture-coordinates-dropped",
                "tessellation.edges-not-transferred",
                "tessellation.mesh-rejected",
                "exact.record-uninterpreted",
                "exact.surface-untyped",
                "exact.surface-as-bspline",
                "exact.curve-untyped",
                "exact.pcurve-untransferred",
                "exact.secondary-pcurve-omitted",
                "exact.endpoint-mismatch",
                "exact.loop-open",
                "exact.face-side-gauge",
                "exact.topology-work-exhausted",
                "exact.edge-uses-merged",
                "exact.face-unbounded-omitted",
                "exact.coedge-degenerate-omitted",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in PvzLossCode::ALL {
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
        for code in PvzLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
