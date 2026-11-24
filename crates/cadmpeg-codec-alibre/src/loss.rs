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
    /// A coedge names a pcurve record with no decodable 2D carrier.
    GeometryPcurveUndecoded,
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
    /// Integers of the extended entity base have no neutral projection.
    SourceRecordExtensionsUnprojected,
    /// The ACIS save format lies outside the verified Spatial ACIS bands.
    SourceDialectUnverified,
    /// Streams of the container carry no decoded content in the IR.
    SourceStreamsUndecoded,
    /// A history state holds an operation record that no feature of the history maps.
    HistoryOperationUnmapped,
    /// A sketch constraint record has a kind or operands that the history does not map.
    HistoryConstraintUnmapped,
    /// A sketch primitive holds a geometry record that the history does not map.
    HistorySketchGeometryUnmapped,
    /// A driving dimension holds annotation geometry that the history does not map.
    HistoryDimensionAnnotationUnmapped,
    /// A feature selects edges by a topology signature that names no edge of the body.
    HistoryEdgeSelectionUnresolved,
    /// A sketch lies on a face that a topology signature names, and the signature names no face of the body.
    HistoryFaceSelectionUnresolved,
    /// A loft holds settings after its section list that the history does not map.
    HistoryLoftSettingsUnmapped,
    /// An end condition holds a logical that the history reads as the reversal of the sketch normal.
    HistoryDirectionFlagAssumed,
    /// The `STEP_DATA` stream holds no Alibre feature history.
    HistoryStreamUnread,
    /// The document profile and presentation records of the stream carry no decoded content in the IR.
    HistoryPresentationUnmapped,
}

impl AlibreLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [AlibreLossCode] = &[
        Self::ContainerNoncanonical,
        Self::GeometryFramedWithoutCarriers,
        Self::GeometryProceduralSurfaceUntyped,
        Self::GeometryPcurveUndecoded,
        Self::HeaderToleranceUnresolved,
        Self::VertexToleranceUnresolved,
        Self::EdgeToleranceUnresolved,
        Self::TopologyShellUnprojected,
        Self::TopologyFaceOwnerUnprojected,
        Self::SourceRecordNameUnresolved,
        Self::SourceRecordExtensionsUnprojected,
        Self::SourceDialectUnverified,
        Self::SourceStreamsUndecoded,
        Self::HistoryOperationUnmapped,
        Self::HistoryConstraintUnmapped,
        Self::HistorySketchGeometryUnmapped,
        Self::HistoryDimensionAnnotationUnmapped,
        Self::HistoryEdgeSelectionUnresolved,
        Self::HistoryFaceSelectionUnresolved,
        Self::HistoryLoftSettingsUnmapped,
        Self::HistoryDirectionFlagAssumed,
        Self::HistoryStreamUnread,
        Self::HistoryPresentationUnmapped,
    ];

    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::ContainerNoncanonical => "container.noncanonical",
            Self::GeometryFramedWithoutCarriers => "geometry.framed-without-carriers",
            Self::GeometryProceduralSurfaceUntyped => "geometry.procedural-surface-untyped",
            Self::GeometryPcurveUndecoded => "geometry.pcurve-undecoded",
            Self::HeaderToleranceUnresolved => "header.tolerance-unresolved",
            Self::VertexToleranceUnresolved => "vertex.tolerance-unresolved",
            Self::EdgeToleranceUnresolved => "edge.tolerance-unresolved",
            Self::TopologyShellUnprojected => "topology.shell-unprojected",
            Self::TopologyFaceOwnerUnprojected => "topology.face-owner-unprojected",
            Self::SourceRecordNameUnresolved => "source.record-name-unresolved",
            Self::SourceRecordExtensionsUnprojected => "source.record-extensions-unprojected",
            Self::SourceDialectUnverified => "source.kernel-dialect-unverified",
            Self::SourceStreamsUndecoded => "source.streams-undecoded",
            Self::HistoryOperationUnmapped => "history.operation-unmapped",
            Self::HistoryConstraintUnmapped => "history.sketch-constraint-unmapped",
            Self::HistorySketchGeometryUnmapped => "history.sketch-geometry-unmapped",
            Self::HistoryDimensionAnnotationUnmapped => "history.dimension-annotation-unmapped",
            Self::HistoryEdgeSelectionUnresolved => "history.edge-selection-unresolved",
            Self::HistoryFaceSelectionUnresolved => "history.face-selection-unresolved",
            Self::HistoryLoftSettingsUnmapped => "history.loft-settings-unmapped",
            Self::HistoryDirectionFlagAssumed => "history.direction-flag-assumed",
            Self::HistoryStreamUnread => "history.stream-unread",
            Self::HistoryPresentationUnmapped => "history.presentation-unmapped",
        }
    }

    /// The severity of this loss.
    const fn severity(self) -> Severity {
        match self {
            Self::GeometryFramedWithoutCarriers => Severity::Blocking,
            Self::ContainerNoncanonical
            | Self::GeometryProceduralSurfaceUntyped
            | Self::GeometryPcurveUndecoded
            | Self::HeaderToleranceUnresolved
            | Self::VertexToleranceUnresolved
            | Self::EdgeToleranceUnresolved
            | Self::TopologyShellUnprojected
            | Self::TopologyFaceOwnerUnprojected
            | Self::SourceRecordNameUnresolved
            | Self::SourceRecordExtensionsUnprojected
            | Self::SourceDialectUnverified
            | Self::SourceStreamsUndecoded
            | Self::HistoryOperationUnmapped
            | Self::HistoryConstraintUnmapped
            | Self::HistorySketchGeometryUnmapped
            | Self::HistoryDimensionAnnotationUnmapped
            | Self::HistoryEdgeSelectionUnresolved
            | Self::HistoryFaceSelectionUnresolved
            | Self::HistoryLoftSettingsUnmapped
            | Self::HistoryDirectionFlagAssumed
            | Self::HistoryStreamUnread
            | Self::HistoryPresentationUnmapped => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::GeometryFramedWithoutCarriers
            | Self::GeometryProceduralSurfaceUntyped
            | Self::GeometryPcurveUndecoded
            | Self::HeaderToleranceUnresolved
            | Self::VertexToleranceUnresolved
            | Self::EdgeToleranceUnresolved
            | Self::TopologyShellUnprojected
            | Self::TopologyFaceOwnerUnprojected
            | Self::SourceRecordExtensionsUnprojected => LossTaxonomy::GeometryNotTransferred,
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::ContainerNoncanonical | Self::SourceRecordNameUnresolved => {
                LossTaxonomy::NoncanonicalSourceSyntax
            }
            Self::SourceStreamsUndecoded => LossTaxonomy::MetadataNotTransferred,
            Self::HistoryOperationUnmapped
            | Self::HistoryConstraintUnmapped
            | Self::HistorySketchGeometryUnmapped
            | Self::HistoryDimensionAnnotationUnmapped
            | Self::HistoryEdgeSelectionUnresolved
            | Self::HistoryFaceSelectionUnresolved
            | Self::HistoryLoftSettingsUnmapped
            | Self::HistoryDirectionFlagAssumed
            | Self::HistoryStreamUnread
            | Self::HistoryPresentationUnmapped => LossTaxonomy::FeatureHistoryRetained,
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
                "geometry.pcurve-undecoded",
                "header.tolerance-unresolved",
                "vertex.tolerance-unresolved",
                "edge.tolerance-unresolved",
                "topology.shell-unprojected",
                "topology.face-owner-unprojected",
                "source.record-name-unresolved",
                "source.record-extensions-unprojected",
                "source.kernel-dialect-unverified",
                "source.streams-undecoded",
                "history.operation-unmapped",
                "history.sketch-constraint-unmapped",
                "history.sketch-geometry-unmapped",
                "history.dimension-annotation-unmapped",
                "history.edge-selection-unresolved",
                "history.face-selection-unresolved",
                "history.loft-settings-unmapped",
                "history.direction-flag-assumed",
                "history.stream-unread",
                "history.presentation-unmapped",
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
