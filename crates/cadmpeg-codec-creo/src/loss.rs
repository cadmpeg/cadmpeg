// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.prt` decoding.
//!
//! Every carrier summary, gap, and drop the decoder reports carries a stable
//! machine-readable code from [`CreoLossCode`]. Codes are the gating surface:
//! harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new loss path without a code does not compile.
//!
//! [`CreoLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate: it fixes the loss category and severity from the
//! code so the two cannot drift apart across sites, and it leaves only the
//! per-instance message to the caller.

use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};

/// A stable, machine-readable identifier for one `.prt` transfer loss.
///
/// Variants are grouped by the decode phase whose transfer degraded. The string
/// form (via [`CreoLossCode::code`]) is the stable contract; the Rust variant
/// name may be refactored freely.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CreoLossCode {
    /// Container-only decode skipped entity transfer.
    ContainerOnlyDecode,
    /// Structural census of the decoded PSB container namespace.
    NamespaceCensusSummary,
    /// General model B-rep transfer is incomplete; sections preserved verbatim.
    GeneralBrepIncomplete,
    /// Model-space plane carriers transferred from `VisibGeom` support frames.
    PlaneCarriersTransferred,
    /// First-instance ND prototype carriers transferred from named parameters.
    FirstInstancePrototypeCarriersTransferred,
    /// Sphere carriers transferred from paired type-26 hemisphere envelopes.
    SphereCarriersTransferred,
    /// Positional torus carriers transferred from complete envelope bodies.
    PositionalTorusCarriersTransferred,
    /// Positional cylinder carriers transferred from per-instance bodies.
    PositionalCylinderCarriersTransferred,
    /// Positional cone carriers transferred from support-apex/envelope bodies.
    PositionalConeCarriersTransferred,
    /// Straight surface-of-extrusion carriers transferred from sweep frames.
    LineExtrusionCarriersTransferred,
    /// Tabulated-cylinder spline extrusion carriers transferred from spans.
    TabulatedCylinderSplineExtrusionCarriersTransferred,
    /// Construction datum plane carriers transferred from `ActDatums`.
    DatumPlaneCarriersTransferred,
    /// Model-space reference line carriers transferred from `MdlRefInfo`.
    ReferenceLineCarriersTransferred,
    /// Circular reference carriers transferred from `MdlRefInfo` rows.
    ReferenceCircleCarriersTransferred,
    /// Elliptical reference carriers transferred from `MdlRefInfo` conic rows.
    ReferenceEllipseCarriersTransferred,
    /// Exact model-space points transferred for native topological vertices.
    TopologicalPointCarriersTransferred,
    /// Native topological edges transferred from exact vertex orbits.
    TopologicalEdgeCarriersTransferred,
    /// Exact line carriers transferred by mapping native linear pcurves.
    PcurveLineCarriersTransferred,
    /// Tagged type-26 torus parameter fields retained as native data.
    TorusParameterCoverageRetained,
    /// Remaining per-instance geometry is gated by unresolved decode layers.
    PerInstanceGeometryGated,
    /// Topology transferred only for components with complete solved boundaries.
    TopologyPartiallyTransferred,
    /// Feature operations and history retained; neutral semantics untransferred.
    FeatureOperationsRetained,
    /// Unique `VisibGeom` surface rows not transferred as carriers.
    VisibleSurfaceRowsUntransferred,
    /// Unique `VisibGeom` curve-topology rows not transferred as carriers.
    VisibleCurveRowsUntransferred,
    /// `VisibGeom` surface rows share a non-unique identity; not resolved.
    AmbiguousSurfaceRows,
    /// `VisibGeom` curve-topology rows share a non-unique identity; not resolved.
    AmbiguousCurveRows,
    /// Active curve-equation records with prohibited constructs retained unvalued.
    ProhibitedCurveExpressionRecordsRetained,
    /// Prohibited datum-curve constructs across curve-equation records unvalued.
    ProhibitedCurveExpressionKindsRetained,
    /// Datum plane in-plane u-axis derived from the normal by convention.
    DatumUAxisInferred,
    /// `VisibGeom` plane local system is an incomplete frame; not a placed carrier.
    IncompletePlaneFrame,
    /// `FeatDefs` sketch record preserved natively with no placed feature.
    UnplacedSketchRecord,
}

impl CreoLossCode {
    /// Every code, in declaration order. Used by tests to assert stability.
    pub const ALL: &'static [CreoLossCode] = &[
        Self::ContainerOnlyDecode,
        Self::NamespaceCensusSummary,
        Self::GeneralBrepIncomplete,
        Self::PlaneCarriersTransferred,
        Self::FirstInstancePrototypeCarriersTransferred,
        Self::SphereCarriersTransferred,
        Self::PositionalTorusCarriersTransferred,
        Self::PositionalCylinderCarriersTransferred,
        Self::PositionalConeCarriersTransferred,
        Self::LineExtrusionCarriersTransferred,
        Self::TabulatedCylinderSplineExtrusionCarriersTransferred,
        Self::DatumPlaneCarriersTransferred,
        Self::ReferenceLineCarriersTransferred,
        Self::ReferenceCircleCarriersTransferred,
        Self::ReferenceEllipseCarriersTransferred,
        Self::TopologicalPointCarriersTransferred,
        Self::TopologicalEdgeCarriersTransferred,
        Self::PcurveLineCarriersTransferred,
        Self::TorusParameterCoverageRetained,
        Self::PerInstanceGeometryGated,
        Self::TopologyPartiallyTransferred,
        Self::FeatureOperationsRetained,
        Self::VisibleSurfaceRowsUntransferred,
        Self::VisibleCurveRowsUntransferred,
        Self::AmbiguousSurfaceRows,
        Self::AmbiguousCurveRows,
        Self::ProhibitedCurveExpressionRecordsRetained,
        Self::ProhibitedCurveExpressionKindsRetained,
        Self::DatumUAxisInferred,
        Self::IncompletePlaneFrame,
        Self::UnplacedSketchRecord,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ContainerOnlyDecode => "container.only-decode",
            Self::NamespaceCensusSummary => "carrier.namespace-census",
            Self::GeneralBrepIncomplete => "geometry.general-brep-incomplete",
            Self::PlaneCarriersTransferred => "carrier.plane-transferred",
            Self::FirstInstancePrototypeCarriersTransferred => {
                "carrier.first-instance-prototype-transferred"
            }
            Self::SphereCarriersTransferred => "carrier.sphere-transferred",
            Self::PositionalTorusCarriersTransferred => "carrier.positional-torus-transferred",
            Self::PositionalCylinderCarriersTransferred => {
                "carrier.positional-cylinder-transferred"
            }
            Self::PositionalConeCarriersTransferred => "carrier.positional-cone-transferred",
            Self::LineExtrusionCarriersTransferred => "carrier.line-extrusion-transferred",
            Self::TabulatedCylinderSplineExtrusionCarriersTransferred => {
                "carrier.tabulated-cylinder-spline-extrusion-transferred"
            }
            Self::DatumPlaneCarriersTransferred => "carrier.datum-plane-transferred",
            Self::ReferenceLineCarriersTransferred => "carrier.reference-line-transferred",
            Self::ReferenceCircleCarriersTransferred => "carrier.reference-circle-transferred",
            Self::ReferenceEllipseCarriersTransferred => "carrier.reference-ellipse-transferred",
            Self::TopologicalPointCarriersTransferred => "carrier.topological-point-transferred",
            Self::TopologicalEdgeCarriersTransferred => "carrier.topological-edge-transferred",
            Self::PcurveLineCarriersTransferred => "carrier.pcurve-line-transferred",
            Self::TorusParameterCoverageRetained => "carrier.torus-parameter-coverage-retained",
            Self::PerInstanceGeometryGated => "geometry.per-instance-gated",
            Self::TopologyPartiallyTransferred => "topology.partially-transferred",
            Self::FeatureOperationsRetained => "feature.operations-retained",
            Self::VisibleSurfaceRowsUntransferred => "geometry.visible-surface-rows-untransferred",
            Self::VisibleCurveRowsUntransferred => "geometry.visible-curve-rows-untransferred",
            Self::AmbiguousSurfaceRows => "geometry.ambiguous-surface-rows",
            Self::AmbiguousCurveRows => "geometry.ambiguous-curve-rows",
            Self::ProhibitedCurveExpressionRecordsRetained => {
                "feature.prohibited-curve-expression-records"
            }
            Self::ProhibitedCurveExpressionKindsRetained => {
                "feature.prohibited-curve-expression-kinds"
            }
            Self::DatumUAxisInferred => "datum.u-axis-inferred",
            Self::IncompletePlaneFrame => "geometry.incomplete-plane-frame",
            Self::UnplacedSketchRecord => "sketch.unplaced-record",
        }
    }

    /// The subsystem category this loss belongs to.
    #[must_use]
    pub const fn category(self) -> LossCategory {
        match self {
            Self::TopologicalEdgeCarriersTransferred | Self::TopologyPartiallyTransferred => {
                LossCategory::Topology
            }
            Self::FeatureOperationsRetained
            | Self::ProhibitedCurveExpressionRecordsRetained
            | Self::ProhibitedCurveExpressionKindsRetained
            | Self::UnplacedSketchRecord => LossCategory::Attribute,
            _ => LossCategory::Geometry,
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::GeneralBrepIncomplete
            | Self::PerInstanceGeometryGated
            | Self::TopologyPartiallyTransferred => Severity::Blocking,
            Self::FeatureOperationsRetained
            | Self::VisibleSurfaceRowsUntransferred
            | Self::VisibleCurveRowsUntransferred
            | Self::ProhibitedCurveExpressionRecordsRetained
            | Self::ProhibitedCurveExpressionKindsRetained => Severity::Warning,
            _ => Severity::Info,
        }
    }

    /// The shared IR loss code this creo loss maps onto.
    const fn shared_code(self) -> LossCode {
        match self {
            Self::ContainerOnlyDecode => LossCode::ContainerOnly,
            Self::NamespaceCensusSummary
            | Self::PlaneCarriersTransferred
            | Self::FirstInstancePrototypeCarriersTransferred
            | Self::SphereCarriersTransferred
            | Self::PositionalTorusCarriersTransferred
            | Self::PositionalCylinderCarriersTransferred
            | Self::PositionalConeCarriersTransferred
            | Self::LineExtrusionCarriersTransferred
            | Self::TabulatedCylinderSplineExtrusionCarriersTransferred
            | Self::DatumPlaneCarriersTransferred
            | Self::ReferenceLineCarriersTransferred
            | Self::ReferenceCircleCarriersTransferred
            | Self::ReferenceEllipseCarriersTransferred
            | Self::TopologicalPointCarriersTransferred
            | Self::TopologicalEdgeCarriersTransferred
            | Self::PcurveLineCarriersTransferred
            | Self::TorusParameterCoverageRetained => LossCode::CarrierSummary,
            Self::GeneralBrepIncomplete
            | Self::PerInstanceGeometryGated
            | Self::VisibleSurfaceRowsUntransferred
            | Self::VisibleCurveRowsUntransferred
            | Self::AmbiguousSurfaceRows
            | Self::AmbiguousCurveRows
            | Self::IncompletePlaneFrame => LossCode::GeometryNotTransferred,
            Self::TopologyPartiallyTransferred => LossCode::TopologyNotTransferred,
            Self::FeatureOperationsRetained
            | Self::ProhibitedCurveExpressionRecordsRetained
            | Self::ProhibitedCurveExpressionKindsRetained => LossCode::FeatureHistoryRetained,
            Self::DatumUAxisInferred => LossCode::CarrierAxisInferred,
            Self::UnplacedSketchRecord => LossCode::PassthroughRecordOmitted,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// Category, severity, and the shared IR code come from the variant, so a
    /// site cannot mislabel a loss it names. Provenance is left absent; the
    /// decoder attributes losses through the message and record identity, not a
    /// source span.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
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
    use super::CreoLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned. A
    /// diff here is an intentional contract change to a gating identifier.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = CreoLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "container.only-decode",
                "carrier.namespace-census",
                "geometry.general-brep-incomplete",
                "carrier.plane-transferred",
                "carrier.first-instance-prototype-transferred",
                "carrier.sphere-transferred",
                "carrier.positional-torus-transferred",
                "carrier.positional-cylinder-transferred",
                "carrier.positional-cone-transferred",
                "carrier.line-extrusion-transferred",
                "carrier.tabulated-cylinder-spline-extrusion-transferred",
                "carrier.datum-plane-transferred",
                "carrier.reference-line-transferred",
                "carrier.reference-circle-transferred",
                "carrier.reference-ellipse-transferred",
                "carrier.topological-point-transferred",
                "carrier.topological-edge-transferred",
                "carrier.pcurve-line-transferred",
                "carrier.torus-parameter-coverage-retained",
                "geometry.per-instance-gated",
                "topology.partially-transferred",
                "feature.operations-retained",
                "geometry.visible-surface-rows-untransferred",
                "geometry.visible-curve-rows-untransferred",
                "geometry.ambiguous-surface-rows",
                "geometry.ambiguous-curve-rows",
                "feature.prohibited-curve-expression-records",
                "feature.prohibited-curve-expression-kinds",
                "datum.u-axis-inferred",
                "geometry.incomplete-plane-frame",
                "sketch.unplaced-record",
            ]
        );
    }

    /// Codes are unique and use the stable `family.detail` kebab shape.
    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in CreoLossCode::ALL {
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

    /// The note builder fixes category and severity from the code so a call
    /// site cannot mislabel a loss it names.
    #[test]
    fn note_takes_category_and_severity_from_the_code() {
        for code in CreoLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.category, code.category());
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
