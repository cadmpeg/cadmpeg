// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for `.prt` decoding.
//!
//! Every carrier summary, gap, and drop the decoder reports carries a stable
//! machine-readable code from [`CreoLossCode`], so a reworded message is not a
//! contract change and a new loss path without a variant does not compile.
//!
//! [`CreoLossCode::note`] is the single construction path for a decode-time
//! [`LossNote`] in this crate: it fixes the loss category and severity from the
//! code so the two cannot drift apart across sites, and it leaves only the
//! per-instance message to the caller.
//!
//! The vocabulary is crate-private: [`CreoLossCode`] never appears in serialized
//! output — the [`LossNote`] carries the shared [`LossCode`] the variant maps
//! to — and no production caller outside this crate reads it.

use cadmpeg_ir::report::{LossCategory, LossCode, LossNote, Severity};

/// A stable, machine-readable identifier for one `.prt` transfer loss.
///
/// Variants are grouped by the decode phase whose transfer degraded, in the
/// order [`crate::decode`] emits them. The string form (via
/// [`CreoLossCode::code`]) is the stable contract; the Rust variant name may be
/// refactored freely.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CreoLossCode {
    /// Container-only decode skipped entity transfer.
    ContainerOnlyDecode,
    /// Structural census of the decoded PSB container namespace.
    NamespaceCensusSummary,
    /// General model B-rep transfer is incomplete; sections preserved verbatim.
    GeneralBrepIncomplete,
    /// Model-space plane carriers transferred from `VisibGeom` support frames.
    PlaneCarriersTransferred,
    /// Plane carriers transferred from solved native-face boundary geometry.
    TopologyBoundPlaneCarriersTransferred,
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
    /// Exact analytic carriers transferred by mapping native linear pcurves.
    AnalyticPcurveCarriersTransferred,
    /// NURBS boundary carriers transferred from extrusion/adjacent-plane contact.
    ExtrusionPlaneBoundaryCurveCarriersTransferred,
    /// NURBS generator carriers transferred from a sweep-direction adjacent plane.
    ExtrusionPlaneSectionGeneratorCarriersTransferred,
    /// Shared NURBS generator carriers transferred from opposed extrusion nets.
    SharedExtrusionGeneratorCarriersTransferred,
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
    /// Declared section segment rows did not decode.
    MissingSectionSegmentRows,
    /// Declared section relation rows did not decode.
    MissingSectionRelationRows,
    /// Section relation tables use the invalid zero allocation count.
    MalformedSectionRelationTables,
    /// Declared section incidence rows did not decode.
    MissingSectionIncidenceRows,
    /// Declared section relation-incidence join rows did not decode.
    MissingSectionRelationIncidenceJoinRows,
    /// Decoded section segments retain native geometry; construction unresolved.
    UnresolvedSectionSegmentGeometry,
    /// Active section incidence constraints retain native operands.
    NativeSectionIncidenceConstraintsRetained,
    /// Active section dimension relations retain native operands.
    NativeSectionDimensionRelationsRetained,
    /// Profile sweep history features retain incomplete construction operands.
    IncompleteSweepFeaturesRetained,
    /// Surface construction history features retain incomplete operands.
    IncompleteSurfaceOperationFeaturesRetained,
    /// Other construction history features retain unresolved neutral operands.
    IncompleteOtherConstructionFeaturesRetained,
    /// Recognized non-sweep history features retain incomplete operands.
    IncompleteRecognizedFeaturesRetained,
    /// History feature definitions retain only source-native semantics.
    NativeOnlyFeatureDefinitionsRetained,
    /// Typed history features retain an explicitly unresolved construction.
    ExplicitlyUnresolvedFeatureConstructionsRetained,
    /// Dimension-driven section solver variables retain unresolved exact values.
    UnresolvedDimensionDrivenSolverVariables,
    /// Solver pre-solve estimates use an unresolved dimension-driven sentinel.
    UnresolvedDimensionDrivenSolverGuesses,
    /// Declared section solver variable rows did not decode.
    MissingSectionSolverVariableRows,
    /// Section dimensions retain native value tokens; scalar encoding unresolved.
    UnresolvedSectionDimensionValues,
    /// Referenced configuration driver tables retain unresolved row semantics.
    UnresolvedConfigurationDriverTables,
    /// Active curve-equation records with prohibited constructs retained unvalued.
    ProhibitedCurveExpressionRecordsRetained,
    /// Curve-equation simultaneous-solve blocks retain equations without values.
    UnresolvedCurveExpressionSolveBlocks,
    /// Curve-equation records retain malformed simultaneous-solve control.
    UnresolvedCurveExpressionSolveControl,
    /// Prohibited datum-curve constructs across curve-equation records unvalued.
    ProhibitedCurveExpressionKindsRetained,
}

impl CreoLossCode {
    /// Every code, in declaration order. Used by tests to assert stability.
    #[cfg(test)]
    pub(crate) const ALL: &'static [CreoLossCode] = &[
        Self::ContainerOnlyDecode,
        Self::NamespaceCensusSummary,
        Self::GeneralBrepIncomplete,
        Self::PlaneCarriersTransferred,
        Self::TopologyBoundPlaneCarriersTransferred,
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
        Self::AnalyticPcurveCarriersTransferred,
        Self::ExtrusionPlaneBoundaryCurveCarriersTransferred,
        Self::ExtrusionPlaneSectionGeneratorCarriersTransferred,
        Self::SharedExtrusionGeneratorCarriersTransferred,
        Self::TorusParameterCoverageRetained,
        Self::PerInstanceGeometryGated,
        Self::TopologyPartiallyTransferred,
        Self::FeatureOperationsRetained,
        Self::VisibleSurfaceRowsUntransferred,
        Self::VisibleCurveRowsUntransferred,
        Self::AmbiguousSurfaceRows,
        Self::AmbiguousCurveRows,
        Self::MissingSectionSegmentRows,
        Self::MissingSectionRelationRows,
        Self::MalformedSectionRelationTables,
        Self::MissingSectionIncidenceRows,
        Self::MissingSectionRelationIncidenceJoinRows,
        Self::UnresolvedSectionSegmentGeometry,
        Self::NativeSectionIncidenceConstraintsRetained,
        Self::NativeSectionDimensionRelationsRetained,
        Self::IncompleteSweepFeaturesRetained,
        Self::IncompleteSurfaceOperationFeaturesRetained,
        Self::IncompleteOtherConstructionFeaturesRetained,
        Self::IncompleteRecognizedFeaturesRetained,
        Self::NativeOnlyFeatureDefinitionsRetained,
        Self::ExplicitlyUnresolvedFeatureConstructionsRetained,
        Self::UnresolvedDimensionDrivenSolverVariables,
        Self::UnresolvedDimensionDrivenSolverGuesses,
        Self::MissingSectionSolverVariableRows,
        Self::UnresolvedSectionDimensionValues,
        Self::UnresolvedConfigurationDriverTables,
        Self::ProhibitedCurveExpressionRecordsRetained,
        Self::UnresolvedCurveExpressionSolveBlocks,
        Self::UnresolvedCurveExpressionSolveControl,
        Self::ProhibitedCurveExpressionKindsRetained,
    ];

    /// The stable string identifier. This is the gating contract.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::ContainerOnlyDecode => "container.only-decode",
            Self::NamespaceCensusSummary => "carrier.namespace-census",
            Self::GeneralBrepIncomplete => "geometry.general-brep-incomplete",
            Self::PlaneCarriersTransferred => "carrier.plane-transferred",
            Self::TopologyBoundPlaneCarriersTransferred => {
                "carrier.topology-bound-plane-transferred"
            }
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
            Self::AnalyticPcurveCarriersTransferred => "carrier.analytic-pcurve-transferred",
            Self::ExtrusionPlaneBoundaryCurveCarriersTransferred => {
                "carrier.extrusion-plane-boundary-curve-transferred"
            }
            Self::ExtrusionPlaneSectionGeneratorCarriersTransferred => {
                "carrier.extrusion-plane-section-generator-transferred"
            }
            Self::SharedExtrusionGeneratorCarriersTransferred => {
                "carrier.shared-extrusion-generator-transferred"
            }
            Self::TorusParameterCoverageRetained => "carrier.torus-parameter-coverage-retained",
            Self::PerInstanceGeometryGated => "geometry.per-instance-gated",
            Self::TopologyPartiallyTransferred => "topology.partially-transferred",
            Self::FeatureOperationsRetained => "feature.operations-retained",
            Self::VisibleSurfaceRowsUntransferred => "geometry.visible-surface-rows-untransferred",
            Self::VisibleCurveRowsUntransferred => "geometry.visible-curve-rows-untransferred",
            Self::AmbiguousSurfaceRows => "geometry.ambiguous-surface-rows",
            Self::AmbiguousCurveRows => "geometry.ambiguous-curve-rows",
            Self::MissingSectionSegmentRows => "feature.missing-section-segment-rows",
            Self::MissingSectionRelationRows => "feature.missing-section-relation-rows",
            Self::MalformedSectionRelationTables => "feature.malformed-section-relation-tables",
            Self::MissingSectionIncidenceRows => "feature.missing-section-incidence-rows",
            Self::MissingSectionRelationIncidenceJoinRows => {
                "feature.missing-section-relation-incidence-join-rows"
            }
            Self::UnresolvedSectionSegmentGeometry => "geometry.unresolved-section-segment",
            Self::NativeSectionIncidenceConstraintsRetained => {
                "feature.native-section-incidence-constraints"
            }
            Self::NativeSectionDimensionRelationsRetained => {
                "feature.native-section-dimension-relations"
            }
            Self::IncompleteSweepFeaturesRetained => "feature.incomplete-sweep-operands",
            Self::IncompleteSurfaceOperationFeaturesRetained => {
                "feature.incomplete-surface-operation-operands"
            }
            Self::IncompleteOtherConstructionFeaturesRetained => {
                "feature.incomplete-other-construction-operands"
            }
            Self::IncompleteRecognizedFeaturesRetained => {
                "feature.incomplete-recognized-feature-operands"
            }
            Self::NativeOnlyFeatureDefinitionsRetained => "feature.native-only-definitions",
            Self::ExplicitlyUnresolvedFeatureConstructionsRetained => {
                "feature.explicitly-unresolved-constructions"
            }
            Self::UnresolvedDimensionDrivenSolverVariables => {
                "feature.unresolved-dimension-driven-variables"
            }
            Self::UnresolvedDimensionDrivenSolverGuesses => {
                "feature.unresolved-dimension-driven-guesses"
            }
            Self::MissingSectionSolverVariableRows => {
                "feature.missing-section-solver-variable-rows"
            }
            Self::UnresolvedSectionDimensionValues => "feature.unresolved-section-dimension-values",
            Self::UnresolvedConfigurationDriverTables => {
                "feature.unresolved-configuration-driver-tables"
            }
            Self::ProhibitedCurveExpressionRecordsRetained => {
                "feature.prohibited-curve-expression-records"
            }
            Self::UnresolvedCurveExpressionSolveBlocks => {
                "feature.unresolved-curve-expression-solve-blocks"
            }
            Self::UnresolvedCurveExpressionSolveControl => {
                "feature.unresolved-curve-expression-solve-control"
            }
            Self::ProhibitedCurveExpressionKindsRetained => {
                "feature.prohibited-curve-expression-kinds"
            }
        }
    }

    /// The shared IR code, subsystem category, and severity for this loss.
    ///
    /// One exhaustive row per variant, with no catch-all arm: a new variant
    /// that is not classified here fails to compile rather than silently
    /// inheriting another loss's category or severity.
    const fn spec(self) -> (LossCode, LossCategory, Severity) {
        use LossCategory as Cat;
        use LossCode as Code;
        use Severity as Sev;
        match self {
            Self::ContainerOnlyDecode => (Code::ContainerOnly, Cat::Geometry, Sev::Info),

            // Carrier summaries: geometry that did transfer exactly.
            Self::NamespaceCensusSummary
            | Self::PlaneCarriersTransferred
            | Self::TopologyBoundPlaneCarriersTransferred
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
            | Self::AnalyticPcurveCarriersTransferred
            | Self::ExtrusionPlaneBoundaryCurveCarriersTransferred
            | Self::ExtrusionPlaneSectionGeneratorCarriersTransferred
            | Self::SharedExtrusionGeneratorCarriersTransferred
            | Self::TorusParameterCoverageRetained => {
                (Code::CarrierSummary, Cat::Geometry, Sev::Info)
            }
            Self::TopologicalEdgeCarriersTransferred => {
                (Code::CarrierSummary, Cat::Topology, Sev::Info)
            }

            // Geometry that could not transfer.
            Self::GeneralBrepIncomplete | Self::PerInstanceGeometryGated => {
                (Code::GeometryNotTransferred, Cat::Geometry, Sev::Blocking)
            }
            Self::VisibleSurfaceRowsUntransferred
            | Self::VisibleCurveRowsUntransferred
            | Self::UnresolvedSectionSegmentGeometry => {
                (Code::GeometryNotTransferred, Cat::Geometry, Sev::Warning)
            }
            Self::AmbiguousSurfaceRows | Self::AmbiguousCurveRows => {
                (Code::GeometryNotTransferred, Cat::Geometry, Sev::Info)
            }

            // Topology that could not transfer.
            Self::TopologyPartiallyTransferred => {
                (Code::TopologyNotTransferred, Cat::Topology, Sev::Blocking)
            }

            // Feature history retained without complete neutral semantics.
            Self::FeatureOperationsRetained
            | Self::MissingSectionSegmentRows
            | Self::MissingSectionRelationRows
            | Self::MalformedSectionRelationTables
            | Self::MissingSectionIncidenceRows
            | Self::MissingSectionRelationIncidenceJoinRows
            | Self::NativeSectionIncidenceConstraintsRetained
            | Self::NativeSectionDimensionRelationsRetained
            | Self::IncompleteSweepFeaturesRetained
            | Self::IncompleteSurfaceOperationFeaturesRetained
            | Self::IncompleteOtherConstructionFeaturesRetained
            | Self::IncompleteRecognizedFeaturesRetained
            | Self::NativeOnlyFeatureDefinitionsRetained
            | Self::ExplicitlyUnresolvedFeatureConstructionsRetained
            | Self::UnresolvedDimensionDrivenSolverVariables
            | Self::UnresolvedDimensionDrivenSolverGuesses
            | Self::MissingSectionSolverVariableRows
            | Self::UnresolvedSectionDimensionValues
            | Self::UnresolvedConfigurationDriverTables
            | Self::ProhibitedCurveExpressionRecordsRetained
            | Self::UnresolvedCurveExpressionSolveBlocks
            | Self::UnresolvedCurveExpressionSolveControl
            | Self::ProhibitedCurveExpressionKindsRetained => {
                (Code::FeatureHistoryRetained, Cat::Attribute, Sev::Warning)
            }
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// Category, severity, and the shared IR code come from the variant, so a
    /// site cannot mislabel a loss it names. Provenance is left absent; the
    /// decoder attributes losses through the message and record identity, not a
    /// source span.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        let (code, category, severity) = self.spec();
        LossNote {
            code,
            category,
            severity,
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
                "carrier.topology-bound-plane-transferred",
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
                "carrier.analytic-pcurve-transferred",
                "carrier.extrusion-plane-boundary-curve-transferred",
                "carrier.extrusion-plane-section-generator-transferred",
                "carrier.shared-extrusion-generator-transferred",
                "carrier.torus-parameter-coverage-retained",
                "geometry.per-instance-gated",
                "topology.partially-transferred",
                "feature.operations-retained",
                "geometry.visible-surface-rows-untransferred",
                "geometry.visible-curve-rows-untransferred",
                "geometry.ambiguous-surface-rows",
                "geometry.ambiguous-curve-rows",
                "feature.missing-section-segment-rows",
                "feature.missing-section-relation-rows",
                "feature.malformed-section-relation-tables",
                "feature.missing-section-incidence-rows",
                "feature.missing-section-relation-incidence-join-rows",
                "geometry.unresolved-section-segment",
                "feature.native-section-incidence-constraints",
                "feature.native-section-dimension-relations",
                "feature.incomplete-sweep-operands",
                "feature.incomplete-surface-operation-operands",
                "feature.incomplete-other-construction-operands",
                "feature.incomplete-recognized-feature-operands",
                "feature.native-only-definitions",
                "feature.explicitly-unresolved-constructions",
                "feature.unresolved-dimension-driven-variables",
                "feature.unresolved-dimension-driven-guesses",
                "feature.missing-section-solver-variable-rows",
                "feature.unresolved-section-dimension-values",
                "feature.unresolved-configuration-driver-tables",
                "feature.prohibited-curve-expression-records",
                "feature.unresolved-curve-expression-solve-blocks",
                "feature.unresolved-curve-expression-solve-control",
                "feature.prohibited-curve-expression-kinds",
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

    /// The note builder takes every classification field from the variant's
    /// table row, so a call site cannot mislabel a loss it names.
    #[test]
    fn note_takes_classification_from_the_code() {
        for code in CreoLossCode::ALL {
            let (shared, category, severity) = code.spec();
            let note = code.note("x");
            assert_eq!(note.code, shared);
            assert_eq!(note.category, category);
            assert_eq!(note.severity, severity);
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }

    /// The `family` prefix of the string code agrees with the shared IR code
    /// the variant maps onto, so the two classifications cannot drift.
    #[test]
    fn code_family_agrees_with_shared_code() {
        use cadmpeg_ir::report::LossCode;
        for code in CreoLossCode::ALL {
            let (shared, ..) = code.spec();
            let family = code.code().split_once('.').expect("family.detail shape").0;
            let expected = match shared {
                LossCode::ContainerOnly => "container",
                LossCode::CarrierSummary => "carrier",
                LossCode::GeometryNotTransferred => "geometry",
                LossCode::TopologyNotTransferred => "topology",
                LossCode::FeatureHistoryRetained => "feature",
                other => panic!("unmapped shared code {other:?} for {}", code.code()),
            };
            assert_eq!(family, expected, "code {} family mismatch", code.code());
        }
    }
}
