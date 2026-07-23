// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for CATIA V5 `.CATPart` decoding.
//!
//! Every fallback, drop, and untransferred-layer report the decoder emits binds
//! a fixed subsystem category and severity to a code drawn from
//! [`CatiaLossCode`]. Fixing the category and severity on the code keeps the
//! many sites that report the same concept from labelling it inconsistently, and
//! stops a new report path from choosing a category and severity that disagree.
//!
//! [`CatiaLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate: it fixes the shared [`LossCode`], the category,
//! and the severity from the variant and leaves only the per-instance message to
//! the caller. The enum is crate-private vocabulary; it carries no stable string
//! identifier of its own because the emitted [`LossNote`] already carries the
//! shared [`LossCode`], which is the gating contract the goldens embed.

use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};

/// A crate-private identifier for one CATIA transfer loss.
///
/// Each variant fixes the shared [`LossCode`], the subsystem [`LossCategory`],
/// and the [`Severity`] of the loss it names, so a site cannot mislabel a loss
/// it reports. Variants that share a [`LossCode`] but disagree on severity (a
/// gauged-but-closed graph versus a graph that never closed) are kept distinct
/// on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatiaLossCode {
    /// Native design objects, object-graph records, and value blocks are
    /// retained; neutral features, parameters, sketch geometry, and history
    /// dependencies remain unresolved.
    NativeDesignDataRetained,
    /// The transferred model retains unresolved curve and surface carriers
    /// without exact procedural constructions.
    UnresolvedCarriers,
    /// Informational census of verbatim vertices and decoded analytic surface
    /// carriers.
    CarrierSummary,
    /// Plane surface records were located but their tag-bridged parameter
    /// records were absent or invalid.
    PlaneRecordsNotDecoded,
    /// Analytic surface records had a non-finite or out-of-range inline payload
    /// and were not decoded.
    AnalyticRecordsInvalid,
    /// Face-local free-form carrier records retain their tag, bounds, and
    /// orientation, but their aliased surface geometry is not transferred.
    FreeformCarriersRetained,
    /// No B-rep geometry was transferred for this storage variant.
    NoGeometryTransferred,
    /// Serialized NURBS caches, persistent cache bindings, materials, and
    /// document metadata are not yet transferred.
    StandardAttributesNotTransferred,
    /// Container-only decode was requested; entity decode was not attempted.
    ContainerOnlyRequested,
    /// The standard-family B-rep boundary graph was not emitted despite detected
    /// face outer-bound runs.
    StandardBoundaryGraphNotEmitted,
    /// The B-rep topology graph was not built for this file.
    TopologyGraphNotBuilt,
    /// The E5 reference graph is closed, but body/shell orientation uses an
    /// incidence-derived gauge.
    E5OrientationGauged,
    /// E5 analytic carriers decoded, but the reference graph could not be
    /// transferred with a closed surface/pcurve/vertex binding.
    E5GraphNotClosed,
    /// The B5 reference graph is closed, but face sense and body kind use a
    /// deterministic topology gauge.
    B5OrientationGauged,
    /// A maximal reference-closed B5 subset was transferred; variant nodes and
    /// unresolved endpoint lifts remain outside the connected graph.
    B5SubsetTransferred,
    /// Object-stream and consolidated NURBS carriers decoded, but the B5
    /// face/loop/pcurve/edge graph did not close.
    B5GraphNotClosed,
    /// The zero-entity B-rep graph is reconstructed, but some referenced-pole
    /// pcurve occurrences remain unresolved.
    ZeroEntityPcurvesUnresolved,
    /// Zero-entity analytic carriers decoded, but the face/loop/coedge/edge/
    /// vertex graph is not yet transferred.
    ZeroEntityGraphNotTransferred,
}

impl CatiaLossCode {
    /// The subsystem category this loss belongs to.
    pub(crate) const fn category(self) -> LossCategory {
        match self {
            Self::NativeDesignDataRetained => LossCategory::DesignIntent,
            Self::UnresolvedCarriers
            | Self::CarrierSummary
            | Self::PlaneRecordsNotDecoded
            | Self::AnalyticRecordsInvalid
            | Self::FreeformCarriersRetained
            | Self::NoGeometryTransferred
            | Self::ContainerOnlyRequested => LossCategory::Geometry,
            Self::StandardAttributesNotTransferred => LossCategory::Attribute,
            Self::StandardBoundaryGraphNotEmitted
            | Self::TopologyGraphNotBuilt
            | Self::E5OrientationGauged
            | Self::E5GraphNotClosed
            | Self::B5OrientationGauged
            | Self::B5SubsetTransferred
            | Self::B5GraphNotClosed
            | Self::ZeroEntityPcurvesUnresolved
            | Self::ZeroEntityGraphNotTransferred => LossCategory::Topology,
        }
    }

    /// The severity of this loss.
    pub(crate) const fn severity(self) -> Severity {
        match self {
            Self::NativeDesignDataRetained
            | Self::UnresolvedCarriers
            | Self::NoGeometryTransferred
            | Self::StandardBoundaryGraphNotEmitted
            | Self::TopologyGraphNotBuilt
            | Self::E5GraphNotClosed
            | Self::B5SubsetTransferred
            | Self::B5GraphNotClosed
            | Self::ZeroEntityGraphNotTransferred => Severity::Blocking,
            Self::CarrierSummary | Self::ContainerOnlyRequested => Severity::Info,
            Self::PlaneRecordsNotDecoded
            | Self::AnalyticRecordsInvalid
            | Self::FreeformCarriersRetained
            | Self::StandardAttributesNotTransferred
            | Self::E5OrientationGauged
            | Self::B5OrientationGauged
            | Self::ZeroEntityPcurvesUnresolved => Severity::Warning,
        }
    }

    /// The shared IR loss kind stored on the emitted [`LossNote`]. This is the
    /// gating identifier the golden reports embed.
    const fn shared_code(self) -> LossCode {
        match self {
            Self::NativeDesignDataRetained => LossCode::FeatureHistoryRetained,
            Self::UnresolvedCarriers
            | Self::PlaneRecordsNotDecoded
            | Self::AnalyticRecordsInvalid
            | Self::FreeformCarriersRetained
            | Self::NoGeometryTransferred => LossCode::GeometryNotTransferred,
            Self::CarrierSummary => LossCode::CarrierSummary,
            Self::StandardAttributesNotTransferred => LossCode::AttributesNotTransferred,
            Self::ContainerOnlyRequested => LossCode::ContainerOnly,
            Self::StandardBoundaryGraphNotEmitted
            | Self::TopologyGraphNotBuilt
            | Self::E5OrientationGauged
            | Self::E5GraphNotClosed
            | Self::B5OrientationGauged
            | Self::B5SubsetTransferred
            | Self::B5GraphNotClosed
            | Self::ZeroEntityPcurvesUnresolved
            | Self::ZeroEntityGraphNotTransferred => LossCode::TopologyNotTransferred,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// Category and severity come from the code, so a site cannot mislabel a
    /// loss it names. Provenance is left absent: the decoder attributes a loss
    /// through its message and the record identity it names, not a source span.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote {
            code: self.shared_code(),
            category: self.category(),
            severity: self.severity(),
            message: message.into(),
            provenance: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CatiaLossCode;

    const ALL: &[CatiaLossCode] = &[
        CatiaLossCode::NativeDesignDataRetained,
        CatiaLossCode::UnresolvedCarriers,
        CatiaLossCode::CarrierSummary,
        CatiaLossCode::PlaneRecordsNotDecoded,
        CatiaLossCode::AnalyticRecordsInvalid,
        CatiaLossCode::FreeformCarriersRetained,
        CatiaLossCode::NoGeometryTransferred,
        CatiaLossCode::StandardAttributesNotTransferred,
        CatiaLossCode::ContainerOnlyRequested,
        CatiaLossCode::StandardBoundaryGraphNotEmitted,
        CatiaLossCode::TopologyGraphNotBuilt,
        CatiaLossCode::E5OrientationGauged,
        CatiaLossCode::E5GraphNotClosed,
        CatiaLossCode::B5OrientationGauged,
        CatiaLossCode::B5SubsetTransferred,
        CatiaLossCode::B5GraphNotClosed,
        CatiaLossCode::ZeroEntityPcurvesUnresolved,
        CatiaLossCode::ZeroEntityGraphNotTransferred,
    ];

    /// The note builder fixes category and severity from the code so a call
    /// site cannot mislabel a loss it names, and leaves provenance absent.
    #[test]
    fn note_takes_category_and_severity_from_the_code() {
        for &code in ALL {
            let note = code.note("x");
            assert_eq!(note.category, code.category());
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
