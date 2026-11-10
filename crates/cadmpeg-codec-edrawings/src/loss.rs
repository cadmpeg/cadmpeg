// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for eDrawings package decoding.
//!
//! Every fallback and drop the decoder reports carries a stable
//! machine-readable code from [`EdrawingsLossCode`]. Codes are the gating
//! surface: downstream tooling keys on them, never on the message text.
//!
//! [`EdrawingsLossCode::note`] is the single construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code, and it leaves only the per-instance message to the
//! caller.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};
use cadmpeg_ir::report::Severity;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("edrawings");

/// A stable, machine-readable identifier for one eDrawings transfer loss.
///
/// The string form (via [`EdrawingsLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum EdrawingsLossCode {
    /// The model stream declares no known stream version.
    SourceDialectUnverified,
    /// The ZIP central directory was unreadable and members were recovered
    /// from local headers.
    ContainerDirectoryRecovered,
    /// The model stream did not open: the package carries no model data.
    StreamMalformed,
    /// Bytes follow the end of the compressed model stream and were not
    /// transferred.
    StreamTrailingBytes,
    /// The inflated model stream broke a record rule: the walk stopped and the
    /// package carries no model data.
    ModelMalformed,
    /// The model stream decoded without any body.
    ModelNoGeometry,
    /// An object reference, slot, or id of the model stream is unusable.
    ModelObjectInvalid,
    /// A shell could not be decoded and was left out of its body.
    ModelShellInvalid,
    /// Body geometry was rebuilt from display meshes: planar faces only.
    GeometryReconstructedFromMesh,
    /// A body is not watertight and became a sheet.
    GeometryOpenBody,
    /// A closed component faced the wrong way for its nesting and its triangles were reversed.
    GeometryBodyReversed,
    /// Degenerate triangles were dropped from a body.
    GeometryDegenerateTriangles,
    /// A body could not be built from its mesh.
    GeometryBodyFailed,
    /// An instance matrix holds a non-finite value; the identity replaced it.
    StructurePlacementInvalid,
    /// Face group colours differ from their body colour; the body colour
    /// replaced them.
    AppearanceFaceColourReduced,
    /// Part-level or display-state colour and transparency overrides were not
    /// transferred.
    AppearanceOverrideDropped,
}

impl EdrawingsLossCode {
    /// Every code, in declaration order.
    #[cfg(test)]
    const ALL: &'static [EdrawingsLossCode] = &[
        Self::SourceDialectUnverified,
        Self::ContainerDirectoryRecovered,
        Self::StreamMalformed,
        Self::StreamTrailingBytes,
        Self::ModelMalformed,
        Self::ModelNoGeometry,
        Self::ModelObjectInvalid,
        Self::ModelShellInvalid,
        Self::GeometryReconstructedFromMesh,
        Self::GeometryOpenBody,
        Self::GeometryBodyReversed,
        Self::GeometryDegenerateTriangles,
        Self::GeometryBodyFailed,
        Self::StructurePlacementInvalid,
        Self::AppearanceFaceColourReduced,
        Self::AppearanceOverrideDropped,
    ];

    /// The stable `family.detail` string of this code.
    #[must_use]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::SourceDialectUnverified => "source.dialect-unverified",
            Self::ContainerDirectoryRecovered => "container.directory-recovered",
            Self::StreamMalformed => "stream.malformed",
            Self::StreamTrailingBytes => "stream.trailing-bytes",
            Self::ModelMalformed => "model.malformed",
            Self::ModelNoGeometry => "model.no-geometry",
            Self::ModelObjectInvalid => "model.object-invalid",
            Self::ModelShellInvalid => "model.shell-invalid",
            Self::GeometryReconstructedFromMesh => "geometry.reconstructed-from-mesh",
            Self::GeometryOpenBody => "geometry.open-body",
            Self::GeometryBodyReversed => "geometry.body-reversed",
            Self::GeometryDegenerateTriangles => "geometry.degenerate-triangles",
            Self::GeometryBodyFailed => "geometry.body-failed",
            Self::StructurePlacementInvalid => "structure.placement-invalid",
            Self::AppearanceFaceColourReduced => "appearance.face-colour-reduced",
            Self::AppearanceOverrideDropped => "appearance.override-dropped",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::StreamTrailingBytes => Severity::Info,
            Self::SourceDialectUnverified
            | Self::ContainerDirectoryRecovered
            | Self::ModelNoGeometry
            | Self::ModelObjectInvalid
            | Self::ModelShellInvalid
            | Self::GeometryReconstructedFromMesh
            | Self::GeometryOpenBody
            | Self::GeometryBodyReversed
            | Self::GeometryDegenerateTriangles
            | Self::GeometryBodyFailed
            | Self::StructurePlacementInvalid
            | Self::AppearanceFaceColourReduced
            | Self::AppearanceOverrideDropped => Severity::Warning,
            Self::StreamMalformed | Self::ModelMalformed => Severity::Blocking,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::ContainerDirectoryRecovered => LossTaxonomy::NoncanonicalSourceSyntax,
            Self::StreamMalformed
            | Self::ModelMalformed
            | Self::ModelNoGeometry
            | Self::ModelShellInvalid
            | Self::GeometryDegenerateTriangles
            | Self::GeometryBodyFailed => LossTaxonomy::GeometryNotTransferred,
            Self::ModelObjectInvalid => LossTaxonomy::DecodeDiagnostic,
            Self::GeometryReconstructedFromMesh
            | Self::GeometryOpenBody
            | Self::GeometryBodyReversed => LossTaxonomy::TopologyGaugeSubstituted,
            Self::StructurePlacementInvalid => LossTaxonomy::AssemblyPlacementsNotTransferred,
            Self::AppearanceFaceColourReduced => LossTaxonomy::AppearanceReduced,
            Self::AppearanceOverrideDropped => LossTaxonomy::MaterialNotTransferred,
            Self::StreamTrailingBytes => LossTaxonomy::RecordNotTyped,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(NAMESPACE, self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `edrawings/<local>`. Severity comes from the
    /// local code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::EdrawingsLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = EdrawingsLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "source.dialect-unverified",
                "container.directory-recovered",
                "stream.malformed",
                "stream.trailing-bytes",
                "model.malformed",
                "model.no-geometry",
                "model.object-invalid",
                "model.shell-invalid",
                "geometry.reconstructed-from-mesh",
                "geometry.open-body",
                "geometry.body-reversed",
                "geometry.degenerate-triangles",
                "geometry.body-failed",
                "structure.placement-invalid",
                "appearance.face-colour-reduced",
                "appearance.override-dropped",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in EdrawingsLossCode::ALL {
            let text = code.code();
            assert!(seen.insert(text), "duplicate code {text}");
            let (family, detail) = text.split_once('.').unwrap();
            assert!(!family.is_empty() && !detail.is_empty());
            assert!(text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'-'));
        }
    }

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in EdrawingsLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
