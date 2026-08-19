// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary for Creo / Pro/ENGINEER `.prt` decoding.
//!
//! Every fallback, approximation, and drop the decoder reports carries a
//! stable machine-readable code from [`CreoLossCode`]. Codes are the gating
//! surface: harness oracles and downstream tooling key on them, never on the
//! human-readable message text, so a reworded message is not a contract change
//! and a new drop path without a code does not compile.
//!
//! [`CreoLossCode::note`] is the single practical construction path for a
//! decode-time [`LossNote`] in this crate: it fixes the loss category and
//! severity from the code so the two cannot drift apart across sites, and it
//! leaves only the per-instance message to the caller.
//!
use cadmpeg_ir::report::{LossKind, LossNote, LossTaxonomy, Severity};

/// A stable, machine-readable identifier for one Creo transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`CreoLossCode::code`]) is the stable contract.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CreoLossCode {
    /// Container-only decode skipped entity transfer.
    ContainerOnlyDecode,
    /// PSB section census and prototype/instance transfer summary.
    ContainerCensus,
    /// Legacy type-2 real row did not form a complete finite scalar or array.
    LegacyRealValueUnresolved,
    /// Legacy type-1 integer row did not form a signed 32-bit scalar or array.
    LegacyIntegerValueUnresolved,
    /// Legacy type-3 or type-4 value row uses an undefined continuation form.
    LegacyContinuationFormUndefined,
    /// Legacy type-3 or type-4 byte-string retains non-UTF-8 source bytes.
    LegacyByteStringEncodingRetained,
    /// Legacy type-5/7/9/11 row did not form an unsigned 32-bit scalar or array.
    LegacyUnsignedValueUnresolved,
    /// Legacy type-6 compact-real row did not form a complete finite scalar or array.
    LegacyCompactRealUnresolved,
    /// Legacy type-0 object array element count differs from declared extents.
    LegacyObjectArrayIncomplete,
    /// Legacy type-0 value row uses an undefined object payload form.
    LegacyObjectPayloadUndefined,
    /// Legacy type-10 string array element count differs from its first extent.
    LegacyStringArrayIncomplete,
    /// Legacy type-10 value row uses an undefined continuation form.
    LegacyStringContinuationUndefined,
    /// Legacy type-10 string element retains non-UTF-8 source bytes.
    LegacyStringEncodingRetained,
    /// Primitive triangle-strip records carry disagreeing position representations.
    TriangleStripRepresentationConflict,
    /// General model B-rep transfer remains incomplete for later instances.
    BrepTransferIncomplete,
    /// Remaining per-instance surfaces, curves, and vertices stay gated.
    GeometryInstanceCarriersGated,
    /// Unique `VisibGeom` surface rows were not transferred as carriers.
    VisibGeomSurfaceUntransferred,
    /// Unique `VisibGeom` curve-topology rows were not transferred as carriers.
    VisibGeomCurveUntransferred,
    /// `VisibGeom` surface rows share a non-unique identity.
    VisibGeomSurfaceAmbiguous,
    /// `VisibGeom` curve-topology rows share a non-unique identity.
    VisibGeomCurveAmbiguous,
    /// Decoded section segments retain source-native geometry.
    SectionSegmentGeometryUnresolved,
    /// Model-space planes transferred from `VisibGeom` local-system frames.
    CarrierVisibGeomPlanes,
    /// Model-space planes transferred from topology-bound constructions.
    CarrierTopologyBoundPlanes,
    /// First-instance ND analytic or interpolation-spline carriers transferred.
    CarrierFirstInstancePrototypes,
    /// Sphere carriers transferred from complementary hemisphere envelopes.
    CarrierPairedEnvelopeSpheres,
    /// Exact positional torus carriers transferred.
    CarrierPositionalTori,
    /// Exact positional cylinder carriers transferred.
    CarrierPositionalCylinders,
    /// Exact positional cone carriers transferred.
    CarrierPositionalCones,
    /// Unbound straight positional surface-of-extrusion carriers transferred.
    CarrierLineExtrusionPlanes,
    /// Tabulated-cylinder cubic spline extrusion carriers transferred.
    CarrierTabulatedCylinderExtrusions,
    /// Exact model-space construction datum planes transferred.
    CarrierDatumPlanes,
    /// Finite model-space reference line carriers transferred.
    CarrierReferenceLines,
    /// Circular reference carriers transferred from `MdlRefInfo`.
    CarrierReferenceCircles,
    /// Elliptical reference carriers transferred from `MdlRefInfo`.
    CarrierReferenceEllipses,
    /// Exact model-space points transferred for topological vertex orbits.
    CarrierTopologicalPoints,
    /// Native topological edges with exact endpoint points transferred.
    CarrierTopologicalEdges,
    /// Exact analytic carriers transferred from native linear pcurves.
    CarrierAnalyticPcurves,
    /// Exact NURBS boundary carriers transferred at extrusion-plane contacts.
    CarrierExtrusionBoundaryCurves,
    /// Exact NURBS generator carriers transferred from plane-section directrices.
    CarrierExtrusionSectionGenerators,
    /// Exact shared NURBS generator carriers transferred between extrusions.
    CarrierSharedExtrusionGenerators,
    /// Tagged torus radius, outline, and envelope fields retained as native data.
    CarrierTorusParameterRetention,
    /// Remaining topology components lack complete face, curve, or vertex data.
    TopologyIncompleteComponents,
    /// Neutral feature, configuration, graph, material, and display data remain open.
    FeatureNeutralSemanticsIncomplete,
    /// Profile sweep history features retain incomplete required operands.
    FeatureSweepIncomplete,
    /// Surface construction history features retain incomplete required operands.
    FeatureSurfaceOperationIncomplete,
    /// Construction history features retain unresolved neutral operands.
    FeatureConstructionIncomplete,
    /// Recognized non-sweep history features retain incomplete required operands.
    FeatureRecognizedIncomplete,
    /// History feature definitions retain only source-native semantics.
    FeatureNativeSemantics,
    /// Typed history features retain an explicitly unresolved construction.
    FeatureConstructionUnresolved,
    /// Declared section segment rows did not decode.
    SectionSegmentMissing,
    /// Declared section relation rows did not decode.
    SectionRelationMissing,
    /// Section relation table uses the invalid zero allocation count.
    SectionRelationTableMalformed,
    /// Declared section incidence rows did not decode.
    SectionIncidenceMissing,
    /// Declared section relation-incidence join rows did not decode.
    SectionRelationJoinMissing,
    /// Active section incidence constraints retain native operands.
    SectionIncidenceNative,
    /// Active section dimension relations retain native operands.
    SectionRelationNative,
    /// Dimension-driven section solver variables retain unresolved exact values.
    SectionDimensionVariableUnresolved,
    /// Section solver pre-solve estimates use an unresolved dimension sentinel.
    SectionDimensionGuessUnresolved,
    /// Declared section solver variable rows did not decode.
    SectionSolverVariableMissing,
    /// Section dimensions retain source-native value tokens.
    SectionDimensionValueUnresolved,
    /// Referenced configuration driver tables retain unresolved traversal.
    ConfigurationDriverUnresolved,
    /// Active curve-equation records with prohibited constructs were not evaluated.
    CurveExpressionProhibited,
    /// Active curve-equation simultaneous-solve blocks retain unsolved equations.
    CurveExpressionSolveUnresolved,
    /// Active curve-equation records retain malformed simultaneous-solve control.
    CurveExpressionSolveControlUnresolved,
    /// Prohibited datum-curve constructs across active curve-equation records.
    CurveExpressionKindProhibited,
}

impl CreoLossCode {
    /// Every code, in declaration order.
    pub const ALL: &'static [CreoLossCode] = &[
        Self::ContainerOnlyDecode,
        Self::ContainerCensus,
        Self::LegacyRealValueUnresolved,
        Self::LegacyIntegerValueUnresolved,
        Self::LegacyContinuationFormUndefined,
        Self::LegacyByteStringEncodingRetained,
        Self::LegacyUnsignedValueUnresolved,
        Self::LegacyCompactRealUnresolved,
        Self::LegacyObjectArrayIncomplete,
        Self::LegacyObjectPayloadUndefined,
        Self::LegacyStringArrayIncomplete,
        Self::LegacyStringContinuationUndefined,
        Self::LegacyStringEncodingRetained,
        Self::TriangleStripRepresentationConflict,
        Self::BrepTransferIncomplete,
        Self::GeometryInstanceCarriersGated,
        Self::VisibGeomSurfaceUntransferred,
        Self::VisibGeomCurveUntransferred,
        Self::VisibGeomSurfaceAmbiguous,
        Self::VisibGeomCurveAmbiguous,
        Self::SectionSegmentGeometryUnresolved,
        Self::CarrierVisibGeomPlanes,
        Self::CarrierTopologyBoundPlanes,
        Self::CarrierFirstInstancePrototypes,
        Self::CarrierPairedEnvelopeSpheres,
        Self::CarrierPositionalTori,
        Self::CarrierPositionalCylinders,
        Self::CarrierPositionalCones,
        Self::CarrierLineExtrusionPlanes,
        Self::CarrierTabulatedCylinderExtrusions,
        Self::CarrierDatumPlanes,
        Self::CarrierReferenceLines,
        Self::CarrierReferenceCircles,
        Self::CarrierReferenceEllipses,
        Self::CarrierTopologicalPoints,
        Self::CarrierTopologicalEdges,
        Self::CarrierAnalyticPcurves,
        Self::CarrierExtrusionBoundaryCurves,
        Self::CarrierExtrusionSectionGenerators,
        Self::CarrierSharedExtrusionGenerators,
        Self::CarrierTorusParameterRetention,
        Self::TopologyIncompleteComponents,
        Self::FeatureNeutralSemanticsIncomplete,
        Self::FeatureSweepIncomplete,
        Self::FeatureSurfaceOperationIncomplete,
        Self::FeatureConstructionIncomplete,
        Self::FeatureRecognizedIncomplete,
        Self::FeatureNativeSemantics,
        Self::FeatureConstructionUnresolved,
        Self::SectionSegmentMissing,
        Self::SectionRelationMissing,
        Self::SectionRelationTableMalformed,
        Self::SectionIncidenceMissing,
        Self::SectionRelationJoinMissing,
        Self::SectionIncidenceNative,
        Self::SectionRelationNative,
        Self::SectionDimensionVariableUnresolved,
        Self::SectionDimensionGuessUnresolved,
        Self::SectionSolverVariableMissing,
        Self::SectionDimensionValueUnresolved,
        Self::ConfigurationDriverUnresolved,
        Self::CurveExpressionProhibited,
        Self::CurveExpressionSolveUnresolved,
        Self::CurveExpressionSolveControlUnresolved,
        Self::CurveExpressionKindProhibited,
    ];

    /// The stable string identifier. This is the gating contract.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ContainerOnlyDecode => "container.decode-skipped",
            Self::ContainerCensus => "container.census",
            Self::LegacyRealValueUnresolved => "legacy.real-value-unresolved",
            Self::LegacyIntegerValueUnresolved => "legacy.integer-value-unresolved",
            Self::LegacyContinuationFormUndefined => "legacy.continuation-form-undefined",
            Self::LegacyByteStringEncodingRetained => "legacy.byte-string-encoding-retained",
            Self::LegacyUnsignedValueUnresolved => "legacy.unsigned-value-unresolved",
            Self::LegacyCompactRealUnresolved => "legacy.compact-real-unresolved",
            Self::LegacyObjectArrayIncomplete => "legacy.object-array-incomplete",
            Self::LegacyObjectPayloadUndefined => "legacy.object-payload-undefined",
            Self::LegacyStringArrayIncomplete => "legacy.string-array-incomplete",
            Self::LegacyStringContinuationUndefined => "legacy.string-continuation-undefined",
            Self::LegacyStringEncodingRetained => "legacy.string-encoding-retained",
            Self::TriangleStripRepresentationConflict => "geometry.triangle-strip-conflict",
            Self::BrepTransferIncomplete => "geometry.brep-incomplete",
            Self::GeometryInstanceCarriersGated => "geometry.instance-carriers-gated",
            Self::VisibGeomSurfaceUntransferred => "geometry.visibgeom-surface-untransferred",
            Self::VisibGeomCurveUntransferred => "geometry.visibgeom-curve-untransferred",
            Self::VisibGeomSurfaceAmbiguous => "geometry.visibgeom-surface-ambiguous",
            Self::VisibGeomCurveAmbiguous => "geometry.visibgeom-curve-ambiguous",
            Self::SectionSegmentGeometryUnresolved => "geometry.section-segment-unresolved",
            Self::CarrierVisibGeomPlanes => "carrier.visibgeom-planes",
            Self::CarrierTopologyBoundPlanes => "carrier.topology-bound-planes",
            Self::CarrierFirstInstancePrototypes => "carrier.first-instance-prototypes",
            Self::CarrierPairedEnvelopeSpheres => "carrier.paired-envelope-spheres",
            Self::CarrierPositionalTori => "carrier.positional-tori",
            Self::CarrierPositionalCylinders => "carrier.positional-cylinders",
            Self::CarrierPositionalCones => "carrier.positional-cones",
            Self::CarrierLineExtrusionPlanes => "carrier.line-extrusion-planes",
            Self::CarrierTabulatedCylinderExtrusions => "carrier.tabulated-cylinder-extrusions",
            Self::CarrierDatumPlanes => "carrier.datum-planes",
            Self::CarrierReferenceLines => "carrier.reference-lines",
            Self::CarrierReferenceCircles => "carrier.reference-circles",
            Self::CarrierReferenceEllipses => "carrier.reference-ellipses",
            Self::CarrierTopologicalPoints => "carrier.topological-points",
            Self::CarrierTopologicalEdges => "carrier.topological-edges",
            Self::CarrierAnalyticPcurves => "carrier.analytic-pcurves",
            Self::CarrierExtrusionBoundaryCurves => "carrier.extrusion-boundary-curves",
            Self::CarrierExtrusionSectionGenerators => "carrier.extrusion-section-generators",
            Self::CarrierSharedExtrusionGenerators => "carrier.shared-extrusion-generators",
            Self::CarrierTorusParameterRetention => "carrier.torus-parameter-retention",
            Self::TopologyIncompleteComponents => "topology.incomplete-components",
            Self::FeatureNeutralSemanticsIncomplete => "feature.neutral-semantics-incomplete",
            Self::FeatureSweepIncomplete => "feature.sweep-incomplete",
            Self::FeatureSurfaceOperationIncomplete => "feature.surface-operation-incomplete",
            Self::FeatureConstructionIncomplete => "feature.construction-incomplete",
            Self::FeatureRecognizedIncomplete => "feature.recognized-incomplete",
            Self::FeatureNativeSemantics => "feature.native-semantics",
            Self::FeatureConstructionUnresolved => "feature.construction-unresolved",
            Self::SectionSegmentMissing => "section.segment-missing",
            Self::SectionRelationMissing => "section.relation-missing",
            Self::SectionRelationTableMalformed => "section.relation-table-malformed",
            Self::SectionIncidenceMissing => "section.incidence-missing",
            Self::SectionRelationJoinMissing => "section.relation-join-missing",
            Self::SectionIncidenceNative => "section.incidence-native",
            Self::SectionRelationNative => "section.relation-native",
            Self::SectionDimensionVariableUnresolved => "section.dimension-variable-unresolved",
            Self::SectionDimensionGuessUnresolved => "section.dimension-guess-unresolved",
            Self::SectionSolverVariableMissing => "section.solver-variable-missing",
            Self::SectionDimensionValueUnresolved => "section.dimension-value-unresolved",
            Self::ConfigurationDriverUnresolved => "configuration.driver-unresolved",
            Self::CurveExpressionProhibited => "curve-expression.prohibited",
            Self::CurveExpressionSolveUnresolved => "curve-expression.solve-unresolved",
            Self::CurveExpressionSolveControlUnresolved => {
                "curve-expression.solve-control-unresolved"
            }
            Self::CurveExpressionKindProhibited => "curve-expression.kind-prohibited",
        }
    }

    /// The severity of this loss.
    #[must_use]
    pub const fn severity(self) -> Severity {
        match self {
            Self::ContainerOnlyDecode
            | Self::ContainerCensus
            | Self::VisibGeomSurfaceAmbiguous
            | Self::VisibGeomCurveAmbiguous
            | Self::CarrierVisibGeomPlanes
            | Self::CarrierTopologyBoundPlanes
            | Self::CarrierFirstInstancePrototypes
            | Self::CarrierPairedEnvelopeSpheres
            | Self::CarrierPositionalTori
            | Self::CarrierPositionalCylinders
            | Self::CarrierPositionalCones
            | Self::CarrierLineExtrusionPlanes
            | Self::CarrierTabulatedCylinderExtrusions
            | Self::CarrierDatumPlanes
            | Self::CarrierReferenceLines
            | Self::CarrierReferenceCircles
            | Self::CarrierReferenceEllipses
            | Self::CarrierTopologicalPoints
            | Self::CarrierTopologicalEdges
            | Self::CarrierAnalyticPcurves
            | Self::CarrierExtrusionBoundaryCurves
            | Self::CarrierExtrusionSectionGenerators
            | Self::CarrierSharedExtrusionGenerators
            | Self::CarrierTorusParameterRetention => Severity::Info,
            Self::BrepTransferIncomplete
            | Self::GeometryInstanceCarriersGated
            | Self::TopologyIncompleteComponents => Severity::Blocking,
            _ => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::ContainerOnlyDecode => LossTaxonomy::ContainerOnly,
            Self::ContainerCensus
            | Self::CarrierVisibGeomPlanes
            | Self::CarrierTopologyBoundPlanes
            | Self::CarrierFirstInstancePrototypes
            | Self::CarrierPairedEnvelopeSpheres
            | Self::CarrierPositionalTori
            | Self::CarrierPositionalCylinders
            | Self::CarrierPositionalCones
            | Self::CarrierLineExtrusionPlanes
            | Self::CarrierTabulatedCylinderExtrusions
            | Self::CarrierDatumPlanes
            | Self::CarrierReferenceLines
            | Self::CarrierReferenceCircles
            | Self::CarrierReferenceEllipses
            | Self::CarrierTopologicalPoints
            | Self::CarrierTopologicalEdges
            | Self::CarrierAnalyticPcurves
            | Self::CarrierExtrusionBoundaryCurves
            | Self::CarrierExtrusionSectionGenerators
            | Self::CarrierSharedExtrusionGenerators
            | Self::CarrierTorusParameterRetention => LossTaxonomy::CarrierSummary,
            Self::LegacyRealValueUnresolved
            | Self::LegacyIntegerValueUnresolved
            | Self::LegacyContinuationFormUndefined
            | Self::LegacyUnsignedValueUnresolved
            | Self::LegacyCompactRealUnresolved
            | Self::LegacyObjectArrayIncomplete
            | Self::LegacyObjectPayloadUndefined
            | Self::LegacyStringArrayIncomplete
            | Self::LegacyStringContinuationUndefined => LossTaxonomy::RecordNotTyped,
            Self::LegacyByteStringEncodingRetained | Self::LegacyStringEncodingRetained => {
                LossTaxonomy::AttributesNotTransferred
            }
            Self::TriangleStripRepresentationConflict
            | Self::BrepTransferIncomplete
            | Self::GeometryInstanceCarriersGated
            | Self::VisibGeomSurfaceUntransferred
            | Self::VisibGeomCurveUntransferred
            | Self::VisibGeomSurfaceAmbiguous
            | Self::VisibGeomCurveAmbiguous
            | Self::SectionSegmentGeometryUnresolved => LossTaxonomy::GeometryNotTransferred,
            Self::TopologyIncompleteComponents => LossTaxonomy::TopologyNotTransferred,
            Self::FeatureNeutralSemanticsIncomplete
            | Self::FeatureSweepIncomplete
            | Self::FeatureSurfaceOperationIncomplete
            | Self::FeatureConstructionIncomplete
            | Self::FeatureRecognizedIncomplete
            | Self::FeatureNativeSemantics
            | Self::FeatureConstructionUnresolved
            | Self::SectionSegmentMissing
            | Self::SectionRelationMissing
            | Self::SectionRelationTableMalformed
            | Self::SectionIncidenceMissing
            | Self::SectionRelationJoinMissing
            | Self::SectionIncidenceNative
            | Self::SectionRelationNative
            | Self::SectionDimensionVariableUnresolved
            | Self::SectionDimensionGuessUnresolved
            | Self::SectionSolverVariableMissing
            | Self::SectionDimensionValueUnresolved
            | Self::ConfigurationDriverUnresolved
            | Self::CurveExpressionProhibited
            | Self::CurveExpressionSolveUnresolved
            | Self::CurveExpressionSolveControlUnresolved
            | Self::CurveExpressionKindProhibited => LossTaxonomy::FeatureHistoryRetained,
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub fn kind(self) -> LossKind {
        LossKind::namespaced("creo", self.code(), self.shared_taxonomy())
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `creo/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests {
    use super::CreoLossCode;
    use std::collections::BTreeSet;

    /// Value-level golden: the stable string form of every code, pinned.
    #[test]
    fn code_strings_are_pinned() {
        let codes: Vec<&str> = CreoLossCode::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(
            codes,
            [
                "container.decode-skipped",
                "container.census",
                "legacy.real-value-unresolved",
                "legacy.integer-value-unresolved",
                "legacy.continuation-form-undefined",
                "legacy.byte-string-encoding-retained",
                "legacy.unsigned-value-unresolved",
                "legacy.compact-real-unresolved",
                "legacy.object-array-incomplete",
                "legacy.object-payload-undefined",
                "legacy.string-array-incomplete",
                "legacy.string-continuation-undefined",
                "legacy.string-encoding-retained",
                "geometry.triangle-strip-conflict",
                "geometry.brep-incomplete",
                "geometry.instance-carriers-gated",
                "geometry.visibgeom-surface-untransferred",
                "geometry.visibgeom-curve-untransferred",
                "geometry.visibgeom-surface-ambiguous",
                "geometry.visibgeom-curve-ambiguous",
                "geometry.section-segment-unresolved",
                "carrier.visibgeom-planes",
                "carrier.topology-bound-planes",
                "carrier.first-instance-prototypes",
                "carrier.paired-envelope-spheres",
                "carrier.positional-tori",
                "carrier.positional-cylinders",
                "carrier.positional-cones",
                "carrier.line-extrusion-planes",
                "carrier.tabulated-cylinder-extrusions",
                "carrier.datum-planes",
                "carrier.reference-lines",
                "carrier.reference-circles",
                "carrier.reference-ellipses",
                "carrier.topological-points",
                "carrier.topological-edges",
                "carrier.analytic-pcurves",
                "carrier.extrusion-boundary-curves",
                "carrier.extrusion-section-generators",
                "carrier.shared-extrusion-generators",
                "carrier.torus-parameter-retention",
                "topology.incomplete-components",
                "feature.neutral-semantics-incomplete",
                "feature.sweep-incomplete",
                "feature.surface-operation-incomplete",
                "feature.construction-incomplete",
                "feature.recognized-incomplete",
                "feature.native-semantics",
                "feature.construction-unresolved",
                "section.segment-missing",
                "section.relation-missing",
                "section.relation-table-malformed",
                "section.incidence-missing",
                "section.relation-join-missing",
                "section.incidence-native",
                "section.relation-native",
                "section.dimension-variable-unresolved",
                "section.dimension-guess-unresolved",
                "section.solver-variable-missing",
                "section.dimension-value-unresolved",
                "configuration.driver-unresolved",
                "curve-expression.prohibited",
                "curve-expression.solve-unresolved",
                "curve-expression.solve-control-unresolved",
                "curve-expression.kind-prohibited",
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

    /// The note builder fixes severity from the codec-specific code.
    #[test]
    fn note_takes_severity_from_the_code() {
        for code in CreoLossCode::ALL {
            let note = code.note("x");
            assert_eq!(note.severity, code.severity());
            assert_eq!(note.message, "x");
            assert!(note.provenance.is_none());
        }
    }
}
