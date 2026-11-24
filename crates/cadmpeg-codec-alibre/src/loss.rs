// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for Alibre Design part decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`AlibreLossCode`]. The string form is the
//! contract. [`AlibreLossCode::note`] is the construction path for a decode-time
//! [`LossNote`]: it fixes the loss category and severity from the code.

use cadmpeg_ir::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy},
    Severity,
};

const NAMESPACE: LossNamespace<'static> = match LossNamespace::new("alibre") {
    Ok(namespace) => namespace,
    Err(_) => panic!("reserved codec namespace"),
};

/// A stable, machine-readable identifier for one Alibre transfer loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum AlibreLossCode {
    /// The compound container uses an admitted noncanonical encoding.
    ContainerNoncanonical,
    /// The ACIS stream framed but decoded no surfaces, points, or faces.
    GeometryFramedWithoutCarriers,
    /// A face rests on a procedural surface construction without a decoded carrier.
    GeometryProceduralSurfaceUntyped,
    /// A header tolerance cannot supply a positive finite document tolerance.
    HeaderToleranceUnresolved,
    /// A tolerant vertex has no positive finite evaluated tolerance.
    VertexToleranceUnresolved,
    /// Tolerant-edge tolerance cannot enter the positive neutral scalar.
    EdgeToleranceUnresolved,
    /// A shell has no admissible member or owner.
    TopologyShellUnprojected,
    /// A standalone source face has no shell owner required by the IR.
    TopologyFaceOwnerUnprojected,
    /// A framed record has no leading source-name component.
    SourceRecordNameUnresolved,
    /// The ACIS save format lies outside the verified Spatial ACIS bands.
    SourceDialectUnverified,
    /// Streams of the container carry no decoded content in the IR.
    SourceStreamsUndecoded,
    /// A component file of an assembly is not available beside the assembly.
    AssemblyMemberUnavailable,
    /// A component file of an assembly is available but does not decode.
    AssemblyMemberUndecoded,
    /// A component file of an assembly contains the assembly that places it.
    AssemblyMemberCycle,
    /// Nested assemblies or occurrences lie past an expansion limit.
    AssemblyExpansionTruncated,
    /// The design records of a component file are not merged into the assembly.
    AssemblyMemberDesignUnmerged,
    /// An inclusion has no readable placement.
    AssemblyPlacementUnreadable,
    /// An inclusion states an attribute that the occurrence does not carry.
    AssemblyInclusionUnmapped,
    /// An inclusion states the hidden state of a component below a subassembly.
    AssemblyNestedStateUnread,
    /// The assembly feature model holds entity families that are not read.
    AssemblyEntitiesUnread,
    /// An occurrence names a component configuration, or the assembly holds more than one configuration, and the IR carries neither.
    AssemblyConfigurationUnmapped,
    /// A component file declares an ACIS save format outside the verified bands.
    AssemblyMemberDialectUnverified,
    /// The file beside the assembly is a different document than the one the inclusion references.
    AssemblyMemberDocumentMismatch,
    /// An assembly record that the reader needs is missing or does not read, and the decode goes on without it.
    AssemblyRecordUnreadable,
}

impl AlibreLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [AlibreLossCode] = &[
        Self::ContainerNoncanonical,
        Self::GeometryFramedWithoutCarriers,
        Self::GeometryProceduralSurfaceUntyped,
        Self::HeaderToleranceUnresolved,
        Self::VertexToleranceUnresolved,
        Self::EdgeToleranceUnresolved,
        Self::TopologyShellUnprojected,
        Self::TopologyFaceOwnerUnprojected,
        Self::SourceRecordNameUnresolved,
        Self::SourceDialectUnverified,
        Self::SourceStreamsUndecoded,
        Self::AssemblyMemberUnavailable,
        Self::AssemblyMemberUndecoded,
        Self::AssemblyMemberCycle,
        Self::AssemblyExpansionTruncated,
        Self::AssemblyMemberDesignUnmerged,
        Self::AssemblyPlacementUnreadable,
        Self::AssemblyInclusionUnmapped,
        Self::AssemblyNestedStateUnread,
        Self::AssemblyEntitiesUnread,
        Self::AssemblyConfigurationUnmapped,
        Self::AssemblyMemberDialectUnverified,
        Self::AssemblyMemberDocumentMismatch,
        Self::AssemblyRecordUnreadable,
    ];

    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::ContainerNoncanonical => "container.noncanonical",
            Self::GeometryFramedWithoutCarriers => "geometry.framed-without-carriers",
            Self::GeometryProceduralSurfaceUntyped => "geometry.procedural-surface-untyped",
            Self::HeaderToleranceUnresolved => "header.tolerance-unresolved",
            Self::VertexToleranceUnresolved => "vertex.tolerance-unresolved",
            Self::EdgeToleranceUnresolved => "edge.tolerance-unresolved",
            Self::TopologyShellUnprojected => "topology.shell-unprojected",
            Self::TopologyFaceOwnerUnprojected => "topology.face-owner-unprojected",
            Self::SourceRecordNameUnresolved => "source.record-name-unresolved",
            Self::SourceDialectUnverified => "source.kernel-dialect-unverified",
            Self::SourceStreamsUndecoded => "source.streams-undecoded",
            Self::AssemblyMemberUnavailable => "assembly.member-unavailable",
            Self::AssemblyMemberUndecoded => "assembly.member-undecoded",
            Self::AssemblyMemberCycle => "assembly.member-cycle",
            Self::AssemblyExpansionTruncated => "assembly.expansion-truncated",
            Self::AssemblyMemberDesignUnmerged => "assembly.member-design-unmerged",
            Self::AssemblyPlacementUnreadable => "assembly.placement-unreadable",
            Self::AssemblyInclusionUnmapped => "assembly.inclusion-field-unmapped",
            Self::AssemblyNestedStateUnread => "assembly.nested-member-state-unread",
            Self::AssemblyEntitiesUnread => "assembly.entities-unread",
            Self::AssemblyConfigurationUnmapped => "assembly.configuration-unmapped",
            Self::AssemblyMemberDialectUnverified => "assembly.member-dialect-unverified",
            Self::AssemblyMemberDocumentMismatch => "assembly.member-document-mismatch",
            Self::AssemblyRecordUnreadable => "assembly.record-unreadable",
        }
    }

    /// The severity of this loss.
    const fn severity(self) -> Severity {
        match self {
            Self::GeometryFramedWithoutCarriers => Severity::Blocking,
            Self::ContainerNoncanonical
            | Self::GeometryProceduralSurfaceUntyped
            | Self::HeaderToleranceUnresolved
            | Self::VertexToleranceUnresolved
            | Self::EdgeToleranceUnresolved
            | Self::TopologyShellUnprojected
            | Self::TopologyFaceOwnerUnprojected
            | Self::SourceRecordNameUnresolved
            | Self::SourceDialectUnverified
            | Self::SourceStreamsUndecoded
            | Self::AssemblyMemberUnavailable
            | Self::AssemblyMemberUndecoded
            | Self::AssemblyMemberCycle
            | Self::AssemblyExpansionTruncated
            | Self::AssemblyMemberDesignUnmerged
            | Self::AssemblyPlacementUnreadable
            | Self::AssemblyInclusionUnmapped
            | Self::AssemblyNestedStateUnread
            | Self::AssemblyEntitiesUnread
            | Self::AssemblyConfigurationUnmapped
            | Self::AssemblyMemberDialectUnverified
            | Self::AssemblyMemberDocumentMismatch
            | Self::AssemblyRecordUnreadable => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::GeometryFramedWithoutCarriers
            | Self::GeometryProceduralSurfaceUntyped
            | Self::HeaderToleranceUnresolved
            | Self::VertexToleranceUnresolved
            | Self::EdgeToleranceUnresolved
            | Self::TopologyShellUnprojected
            | Self::TopologyFaceOwnerUnprojected => LossTaxonomy::GeometryNotTransferred,
            Self::SourceDialectUnverified | Self::AssemblyMemberDialectUnverified => {
                LossTaxonomy::SourceDialectUnverified
            }
            Self::ContainerNoncanonical | Self::SourceRecordNameUnresolved => {
                LossTaxonomy::NoncanonicalSourceSyntax
            }
            Self::SourceStreamsUndecoded => LossTaxonomy::MetadataNotTransferred,
            Self::AssemblyMemberUnavailable
            | Self::AssemblyMemberUndecoded
            | Self::AssemblyMemberCycle
            | Self::AssemblyExpansionTruncated
            | Self::AssemblyMemberDesignUnmerged
            | Self::AssemblyMemberDocumentMismatch => LossTaxonomy::AssemblyComponentsExternal,
            Self::AssemblyPlacementUnreadable => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::AssemblyInclusionUnmapped
            | Self::AssemblyNestedStateUnread
            | Self::AssemblyEntitiesUnread
            | Self::AssemblyRecordUnreadable => LossTaxonomy::ObjectRecordsUntransferred,
            Self::AssemblyConfigurationUnmapped => LossTaxonomy::MetadataNotTransferred,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(NAMESPACE, self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::AlibreLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = AlibreLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "container.noncanonical",
                "geometry.framed-without-carriers",
                "geometry.procedural-surface-untyped",
                "header.tolerance-unresolved",
                "vertex.tolerance-unresolved",
                "edge.tolerance-unresolved",
                "topology.shell-unprojected",
                "topology.face-owner-unprojected",
                "source.record-name-unresolved",
                "source.kernel-dialect-unverified",
                "source.streams-undecoded",
                "assembly.member-unavailable",
                "assembly.member-undecoded",
                "assembly.member-cycle",
                "assembly.expansion-truncated",
                "assembly.member-design-unmerged",
                "assembly.placement-unreadable",
                "assembly.inclusion-field-unmapped",
                "assembly.nested-member-state-unread",
                "assembly.entities-unread",
                "assembly.configuration-unmapped",
                "assembly.member-dialect-unverified",
                "assembly.member-document-mismatch",
                "assembly.record-unreadable",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in AlibreLossCode::ALL {
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
        for code in AlibreLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
        }
    }
}
