// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for Rhino `.3dm` decoding and export.
//!
//! Every fallback, retention, and precision drop the codec reports names one
//! [`RhinoLossCode`] variant. The variant fixes the shared [`LossCode`],
//! category, and severity of the emitted [`LossNote`], so those three cannot
//! drift apart across the sites that report the same kind of loss, and a new
//! drop path must name a variant to compile.
//!
//! [`RhinoLossCode::note`] and [`RhinoLossCode::note_with_provenance`] are the
//! construction paths for a decode- or export-time [`LossNote`] in this crate.
//! Each leaves only the per-instance message — and, where the degraded record
//! carries a source span, its provenance — to the caller.

use cadmpeg_ir::provenance::Provenance;
use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};

/// A named Rhino transfer loss.
///
/// The variant is the classification; [`shared_code`](Self::shared_code),
/// [`category`](Self::category), and [`severity`](Self::severity) are its fixed
/// projection onto the report model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum RhinoLossCode {
    /// Summary of decoded versus total object records.
    ObjectRecordsSummary,
    /// Object records for a class were retained without decoded geometry.
    ObjectFamilyRetained,
    /// Object records for a class kept only degraded attributes.
    ObjectAttributesDegraded,
    /// Framed object records for a class could not be decoded.
    FramedObjectUndecoded,
    /// Instance-definition records were retained malformed, ambiguous, or
    /// checksum-degraded.
    InstanceDefinitionsRetained,
    /// A recoverable container scan warning was carried into the report.
    ScanWarning,
    /// Per-phase decode warnings were aggregated into one note.
    PhaseWarning,
    /// A B-rep record fell back from full topology transfer.
    BrepTopologyFallback,
    /// Standalone mesh vertices were stored at f32 precision.
    MeshVertexQuantized,
    /// Mesh normals were stored at f32 precision.
    MeshNormalQuantized,
}

impl RhinoLossCode {
    /// The subsystem category this loss belongs to.
    pub(crate) const fn category(self) -> LossCategory {
        match self {
            Self::ObjectRecordsSummary
            | Self::ObjectFamilyRetained
            | Self::MeshVertexQuantized
            | Self::MeshNormalQuantized => LossCategory::Geometry,
            Self::ObjectAttributesDegraded => LossCategory::Attribute,
            Self::BrepTopologyFallback => LossCategory::Topology,
            Self::FramedObjectUndecoded
            | Self::InstanceDefinitionsRetained
            | Self::ScanWarning
            | Self::PhaseWarning => LossCategory::Other,
        }
    }

    /// The severity of this loss.
    pub(crate) const fn severity(self) -> Severity {
        match self {
            Self::ObjectRecordsSummary => Severity::Info,
            Self::FramedObjectUndecoded => Severity::Error,
            Self::ObjectFamilyRetained
            | Self::ObjectAttributesDegraded
            | Self::InstanceDefinitionsRetained
            | Self::ScanWarning
            | Self::PhaseWarning
            | Self::BrepTopologyFallback
            | Self::MeshVertexQuantized
            | Self::MeshNormalQuantized => Severity::Warning,
        }
    }

    /// The shared IR loss code carried on the emitted [`LossNote`].
    pub(crate) const fn shared_code(self) -> LossCode {
        match self {
            Self::ObjectRecordsSummary => LossCode::ObjectRecordsUntransferred,
            Self::ObjectFamilyRetained => LossCode::UnsupportedObjectFamily,
            Self::ObjectAttributesDegraded => LossCode::AttributesNotTransferred,
            Self::FramedObjectUndecoded
            | Self::InstanceDefinitionsRetained
            | Self::ScanWarning
            | Self::PhaseWarning => LossCode::DecodeDiagnostic,
            Self::BrepTopologyFallback => LossCode::TopologyNotTransferred,
            Self::MeshVertexQuantized | Self::MeshNormalQuantized => LossCode::MeshVertexPrecision,
        }
    }

    /// Build a [`LossNote`] for this code with no source-span provenance.
    ///
    /// Shared code, category, and severity come from the variant, so a site
    /// cannot mislabel a loss it names.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        self.note_inner(message.into(), None)
    }

    /// Build a [`LossNote`] for this code carrying a record's source-span
    /// provenance.
    pub(crate) fn note_with_provenance(
        self,
        message: impl Into<String>,
        provenance: Provenance,
    ) -> LossNote {
        self.note_inner(message.into(), Some(provenance))
    }

    fn note_inner(self, message: String, provenance: Option<Provenance>) -> LossNote {
        LossNote {
            code: self.shared_code(),
            category: self.category(),
            severity: self.severity(),
            message,
            provenance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RhinoLossCode;

    /// Every variant, so the projection assertions below stay exhaustive when a
    /// variant is added.
    const ALL: &[RhinoLossCode] = &[
        RhinoLossCode::ObjectRecordsSummary,
        RhinoLossCode::ObjectFamilyRetained,
        RhinoLossCode::ObjectAttributesDegraded,
        RhinoLossCode::FramedObjectUndecoded,
        RhinoLossCode::InstanceDefinitionsRetained,
        RhinoLossCode::ScanWarning,
        RhinoLossCode::PhaseWarning,
        RhinoLossCode::BrepTopologyFallback,
        RhinoLossCode::MeshVertexQuantized,
        RhinoLossCode::MeshNormalQuantized,
    ];

    /// Both builders fix shared code, category, and severity from the variant so
    /// a call site cannot mislabel a loss it names; only provenance differs.
    #[test]
    fn note_builders_take_code_category_and_severity_from_the_variant() {
        let provenance = cadmpeg_ir::provenance::Provenance {
            format: "rhino".to_string(),
            stream: String::new(),
            offset: 7,
            tag: Some("TAG".to_string()),
        };
        for code in ALL {
            let bare = code.note("x");
            assert_eq!(bare.code, code.shared_code());
            assert_eq!(bare.category, code.category());
            assert_eq!(bare.severity, code.severity());
            assert_eq!(bare.message, "x");
            assert!(bare.provenance.is_none());

            let attributed = code.note_with_provenance("y", provenance.clone());
            assert_eq!(attributed.code, code.shared_code());
            assert_eq!(attributed.category, code.category());
            assert_eq!(attributed.severity, code.severity());
            assert_eq!(attributed.message, "y");
            assert_eq!(attributed.provenance.as_ref(), Some(&provenance));
        }
    }
}
