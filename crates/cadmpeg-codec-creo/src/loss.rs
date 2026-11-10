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
use cadmpeg_ir::report::{
    loss::{LossKind, LossNote, LossTaxonomy},
    Severity,
};

macro_rules! loss_codes {
    ($(#[$enum_attribute:meta])* pub(crate) enum $name:ident {
        $($(#[$variant_attribute:meta])* $variant:ident),* $(,)?
    }) => {
        $(#[$enum_attribute])*
        pub(crate) enum $name {
            $($(#[$variant_attribute])* $variant),*
        }

        impl $name {
            /// Every code, in declaration order.
            #[cfg(test)]
            const ALL: &'static [Self] = &[$(Self::$variant),*];
        }
    };
}

loss_codes! {
/// A stable, machine-readable identifier for one Creo transfer loss.
///
/// Variants are grouped by the record family whose transfer degraded. The
/// string form (via [`CreoLossCode::code`]) is the stable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CreoLossCode {
    /// PSB section census and prototype/instance transfer summary.
    ContainerCensus,
    /// A byte-bounded compressed section has invalid or incomplete LZW data.
    CompressedSectionUnexpanded,
    /// Geometry-looking bytes in a physical section have no visible-namespace ownership.
    PhysicalGeometryOwnerUnresolved,
    /// A positional ellipse flip is not a complete compact integer.
    ReferenceConicPrefixUnresolved,
    /// No persistence-layout discriminant matched, so no layout-specific
    /// decode strategy was applied.
    SourceDialectUnverified,
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
    /// PMI datum or target associations remain unresolved.
    PmiAssociationsUnresolved,
    /// The neutral PMI attribute or annotation-element table is incomplete.
    PmiTableIncomplete,
    /// A PMI type, value, or owner join is unresolved.
    PmiRecordUnresolved,
    /// PMI modifiers or geometric-tolerance zone constraints remain unresolved.
    PmiModifiersUnresolved,
    /// A PMI length value uses the default millimetre assumption.
    PmiLengthUnitAssumed,
    /// A PMI attribute uses the model length unit or stored radians.
    PmiAttributeUnitAssumed,
    /// A composite tolerance segment uses its formatted magnitude.
    PmiTextMagnitudeInferred,
    /// Datum references with the same label use an inferred shared identity.
    PmiDatumIdentityInferred,
    /// A datum-reference label could not be decoded.
    PmiDatumLabelUnresolved,
    /// Conflicting same-owner attributes use the first stored value.
    PmiFirstAttributeAssumed,
    /// A datum-tag leader is interpreted as the target of a datum feature.
    PmiDatumTargetInferred,
    /// A tolerance's surface attachment selects its unique bounded face.
    PmiToleranceTargetInferred,
    /// A dimension's surface or edge references select its target faces.
    PmiDimensionTargetInferred,
    /// A surface link names a visible surface with faces and a datum surface.
    PmiTargetNamespaceAmbiguous,
    /// A datum tag's definition or geometric target remains unresolved.
    PmiDatumTargetUnresolved,
    /// A datum-tag prototype uses a class-qualified boundary candidate.
    PmiDatumTagBoundaryAssumed,
    /// Datum-tag identities use a candidate datum-definition row boundary.
    PmiDatumDefinitionSlotAssumed,
    /// A dimension kind or its quantity is inferred.
    PmiDimensionKindInferred,
    /// An angular dimension uses the default stored-radian assumption.
    PmiAngularUnitAssumed,
    /// Primitive triangle-strip records carry disagreeing position representations.
    TriangleStripRepresentationConflict,
    /// General model B-rep transfer remains incomplete for later instances.
    BrepTransferIncomplete,
    /// A compact-frame component has unresolved source faces.
    BrepCompactFrameComponentUnresolved,
    BrepCompactFrameBoundaryUnresolved,
    /// A saved section spline cannot form a NURBS curve.
    SectionSplineUnresolved,
    /// A named surface-prototype field's bounded scalar body states no slots
    /// and is retained opaque.
    SurfacePrototypeFieldRetained,
    /// A compact plane rotation has no complete finite origin.
    PlaneCompactFrameUnresolved,
    /// A selected angular wall chart fails complete scalar-field admission, or
    /// it and a support-frame production both reproduce the plane envelope with
    /// different charts.
    PlaneAngularFrameUnresolved,
    /// A bounded angular endpoint body has an unresolved signed literal.
    AngularEndpointLiteralUnresolved,
    /// A complete rotation cylinder conflicts with its stored envelope.
    CylinderRotationEnvelopeUnresolved,
    ActiveBoundaryPlaneUnresolved,
    RevolutionCarrierUnresolved,
    RevolutionBoundaryIntervalUnresolved,
    /// A resolved extrusion body failed shell admission.
    ExtrusionBodyRejected,
    /// Remaining per-instance surfaces, curves, and vertices stay gated.
    GeometryInstanceCarriersGated,
    /// Unique `VisibGeom` surface rows were not transferred as carriers.
    VisibGeomSurfaceUntransferred,
    /// A bounded plane local system has no complete admitted scalar frame.
    PlaneLocalSystemUnresolved,
    PlaneEnvelopeUnresolved,
    PlaneCarrierUnresolved,
    PrototypeLocalSystemUnresolved,
    /// A cylinder rotation frame fails its bounded envelope gate.
    CylinderRotationFrameUnresolved,
    TwoChartScalarFrameUnresolved,
    /// A positional pcurve has no complete bounded scalar frame.
    PcurveScalarFrameUnresolved,
    PcurveChartUnresolved,
    SampledPcurveCarrierUnresolved,
    CylinderIntersectionPcurveUnresolved,
    /// Model curves transferred from rows that only their sample count frames.
    CountFramedCurveCarrierUnverified,
    HoleCircleIntervalUnresolved,
    HoleGeneratorUnresolved,
    SweepEnvelopeReplayUnresolved,
    PcurveSampleCoordinatesDropped,
    AngularPcurvePairUnresolved,
    SampledPcurveEvaluationUnresolved,
    /// Unique `VisibGeom` curve-topology rows were not transferred as carriers.
    VisibGeomCurveUntransferred,
    /// Cross-section curve parameter bodies retain unclassified bytes.
    CrossSectionCurveBodyOpaque,
    /// A generated round entry has no independently owned surface carrier.
    RoundCarrierOwnerUnresolved,
    /// A legacy ASCII surface carrier refused a lane, so its geometry stays a
    /// structural record.
    LegacySurfaceCarrierUnresolved,
    /// A NURBS boundary or plane-generator carrier refused a lane, so the
    /// curve named by its surface row stays a structural record.
    NurbsBoundaryCarrierUnresolved,
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
    CarrierSweepEnvelopeReplays,
    CarrierSampledSweepCircles,
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
    /// A face component whose faces have no visible surface row stays native.
    TopologyUnownedComponent,
    /// A visible face row with a null body and quilt owner stays native.
    TopologyExplicitUnownedSurface,
    /// A half-edge orbit is past the stated topological vertex identifier width.
    TopologyVertexIdentifierUnstatable,
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
    /// Same-ID operation candidates disagree and have no current-state selector.
    FeatureStateSelectionUnresolved,
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
    /// Full-width section variable lanes retain opaque scalar roles.
    SectionVariableScalarOpaque,
    /// Declared section solver variable rows did not decode.
    SectionSolverVariableMissing,
    /// Section dimensions retain source-native value tokens.
    SectionDimensionValueUnresolved,
    /// A native radius cannot form a finite neutral diameter value.
    SectionDiameterValueUnresolved,
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
    /// Plane-support lane search stopped at its local variant or visit cap.
    PlaneSupportVariantsCapped,
    /// A section distance component had more sign branches than the local cap.
    SectionSignBranchesCapped,
    /// Curve-equation relation values were dropped at a local string-text cap.
    CurveExpressionTextCapped,
    /// TOC headers past the input length in visited header and row bytes
    /// were not read.
    TocScanCapped,
    /// Component occurrences reference model files the decode does not open.
    AssemblyComponentsExternal,
    /// The component table is missing or ends before its declared rows.
    AssemblyMemberTableIncomplete,
    /// A component frame is not a proper rigid placement.
    AssemblyPlacementUnresolved,
    /// A component row names no model file the row grammar resolves.
    AssemblyMemberReferenceUnresolved,
    /// Component placement constraints are not decoded.
    AssemblyConstraintsNotTransferred,
}
}

impl CreoLossCode {
    /// The stable string identifier. This is the gating contract.
    #[must_use]
    const fn code(self) -> &'static str {
        match self {
            Self::ContainerCensus => "container.census",
            Self::CompressedSectionUnexpanded => "container.compressed-section-unexpanded",
            Self::PhysicalGeometryOwnerUnresolved => "container.physical-geometry-owner-unresolved",
            Self::ReferenceConicPrefixUnresolved => "reference.conic-prefix-unresolved",
            Self::SourceDialectUnverified => "source.dialect-unverified",
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
            Self::PmiAssociationsUnresolved => "pmi.associations-unresolved",
            Self::PmiTableIncomplete => "pmi.table-incomplete",
            Self::PmiRecordUnresolved => "pmi.record-unresolved",
            Self::PmiModifiersUnresolved => "pmi.modifiers-unresolved",
            Self::PmiLengthUnitAssumed => "pmi.length-unit-assumed",
            Self::PmiAttributeUnitAssumed => "pmi.attribute-unit-assumed",
            Self::PmiTextMagnitudeInferred => "pmi.text-magnitude-inferred",
            Self::PmiDatumIdentityInferred => "pmi.datum-identity-inferred",
            Self::PmiDatumLabelUnresolved => "pmi.datum-label-unresolved",
            Self::PmiFirstAttributeAssumed => "pmi.first-attribute-assumed",
            Self::PmiDatumTargetInferred => "pmi.datum-target-inferred",
            Self::PmiToleranceTargetInferred => "pmi.tolerance-target-inferred",
            Self::PmiDimensionTargetInferred => "pmi.dimension-target-inferred",
            Self::PmiTargetNamespaceAmbiguous => "pmi.target-namespace-ambiguous",
            Self::PmiDatumTargetUnresolved => "pmi.datum-target-unresolved",
            Self::PmiDatumTagBoundaryAssumed => "pmi.datum-tag-boundary-assumed",
            Self::PmiDatumDefinitionSlotAssumed => "pmi.datum-definition-slot-assumed",
            Self::PmiDimensionKindInferred => "pmi.dimension-kind-inferred",
            Self::PmiAngularUnitAssumed => "pmi.angular-unit-assumed",
            Self::TriangleStripRepresentationConflict => "geometry.triangle-strip-conflict",
            Self::BrepTransferIncomplete => "geometry.brep-incomplete",
            Self::BrepCompactFrameComponentUnresolved => {
                "geometry.brep-compact-frame-component-unresolved"
            }
            Self::BrepCompactFrameBoundaryUnresolved => {
                "geometry.brep-compact-frame-boundary-unresolved"
            }
            Self::SectionSplineUnresolved => "geometry.section-spline-unresolved",
            Self::SurfacePrototypeFieldRetained => "geometry.surface-prototype-field-retained",
            Self::SweepEnvelopeReplayUnresolved => "geometry.sweep-envelope-replay-unresolved",
            Self::PcurveSampleCoordinatesDropped => "geometry.pcurve-sample-coordinates-dropped",
            Self::AngularPcurvePairUnresolved => "geometry.angular-pcurve-pair-unresolved",
            Self::PlaneCompactFrameUnresolved => "geometry.plane-compact-frame-unresolved",
            Self::PlaneAngularFrameUnresolved => "geometry.plane-angular-frame-unresolved",
            Self::AngularEndpointLiteralUnresolved => {
                "geometry.angular-endpoint-literal-unresolved"
            }
            Self::CylinderRotationEnvelopeUnresolved => {
                "geometry.cylinder-rotation-envelope-unresolved"
            }
            Self::ActiveBoundaryPlaneUnresolved => "geometry.active-boundary-plane-unresolved",
            Self::RevolutionCarrierUnresolved => "geometry.revolution-carrier-unresolved",
            Self::RevolutionBoundaryIntervalUnresolved => {
                "geometry.revolution-boundary-interval-unresolved"
            }
            Self::ExtrusionBodyRejected => "topology.extrusion-body-rejected",
            Self::GeometryInstanceCarriersGated => "geometry.instance-carriers-gated",
            Self::VisibGeomSurfaceUntransferred => "geometry.visibgeom-surface-untransferred",
            Self::PlaneLocalSystemUnresolved => "geometry.plane-local-system-unresolved",
            Self::PlaneEnvelopeUnresolved => "geometry.plane-envelope-unresolved",
            Self::PlaneCarrierUnresolved => "geometry.plane-carrier-unresolved",
            Self::PrototypeLocalSystemUnresolved => "geometry.prototype-local-system-unresolved",
            Self::CylinderRotationFrameUnresolved => "geometry.cylinder-rotation-frame-unresolved",
            Self::TwoChartScalarFrameUnresolved => "geometry.two-chart-scalar-frame-unresolved",
            Self::PcurveScalarFrameUnresolved => "geometry.pcurve-scalar-frame-unresolved",
            Self::PcurveChartUnresolved => "geometry.pcurve-chart-unresolved",
            Self::SampledPcurveCarrierUnresolved => "geometry.sampled-pcurve-carrier-unresolved",
            Self::CylinderIntersectionPcurveUnresolved => {
                "geometry.cylinder-intersection-pcurve-unresolved"
            }
            Self::CountFramedCurveCarrierUnverified => {
                "geometry.count-framed-curve-carrier-unverified"
            }
            Self::HoleCircleIntervalUnresolved => "geometry.hole-circle-interval-unresolved",
            Self::HoleGeneratorUnresolved => "geometry.hole-generator-unresolved",
            Self::SampledPcurveEvaluationUnresolved => {
                "geometry.sampled-pcurve-evaluation-unresolved"
            }
            Self::VisibGeomCurveUntransferred => "geometry.visibgeom-curve-untransferred",
            Self::CrossSectionCurveBodyOpaque => "curve.cross-section-body-opaque",
            Self::RoundCarrierOwnerUnresolved => "geometry.round-carrier-owner-unresolved",
            Self::LegacySurfaceCarrierUnresolved => "geometry.legacy-surface-carrier-unresolved",
            Self::NurbsBoundaryCarrierUnresolved => "geometry.nurbs-boundary-carrier-unresolved",
            Self::VisibGeomSurfaceAmbiguous => "geometry.visibgeom-surface-ambiguous",
            Self::VisibGeomCurveAmbiguous => "geometry.visibgeom-curve-ambiguous",
            Self::SectionSegmentGeometryUnresolved => "geometry.section-segment-unresolved",
            Self::CarrierVisibGeomPlanes => "carrier.visibgeom-planes",
            Self::CarrierTopologyBoundPlanes => "carrier.topology-bound-planes",
            Self::CarrierFirstInstancePrototypes => "carrier.first-instance-prototypes",
            Self::CarrierPairedEnvelopeSpheres => "carrier.paired-envelope-spheres",
            Self::CarrierSweepEnvelopeReplays => "carrier.sweep-envelope-replays",
            Self::CarrierSampledSweepCircles => "carrier.sampled-sweep-circles",
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
            Self::TopologyUnownedComponent => "topology.unowned-component",
            Self::TopologyExplicitUnownedSurface => "topology.explicit-unowned-surface",
            Self::TopologyVertexIdentifierUnstatable => "topology.vertex-identifier-unstatable",
            Self::FeatureNeutralSemanticsIncomplete => "feature.neutral-semantics-incomplete",
            Self::FeatureSweepIncomplete => "feature.sweep-incomplete",
            Self::FeatureSurfaceOperationIncomplete => "feature.surface-operation-incomplete",
            Self::FeatureConstructionIncomplete => "feature.construction-incomplete",
            Self::FeatureRecognizedIncomplete => "feature.recognized-incomplete",
            Self::FeatureNativeSemantics => "feature.native-semantics",
            Self::FeatureStateSelectionUnresolved => "feature.state-selection-unresolved",
            Self::FeatureConstructionUnresolved => "feature.construction-unresolved",
            Self::SectionSegmentMissing => "section.segment-missing",
            Self::SectionRelationMissing => "section.relation-missing",
            Self::SectionRelationTableMalformed => "section.relation-table-malformed",
            Self::SectionIncidenceMissing => "section.incidence-missing",
            Self::SectionRelationJoinMissing => "section.relation-join-missing",
            Self::SectionIncidenceNative => "section.incidence-native",
            Self::SectionRelationNative => "section.relation-native",
            Self::SectionVariableScalarOpaque => "section.variable-scalar-opaque",
            Self::SectionSolverVariableMissing => "section.solver-variable-missing",
            Self::SectionDimensionValueUnresolved => "section.dimension-value-unresolved",
            Self::SectionDiameterValueUnresolved => "section.diameter-value-unresolved",
            Self::ConfigurationDriverUnresolved => "configuration.driver-unresolved",
            Self::CurveExpressionProhibited => "curve-expression.prohibited",
            Self::CurveExpressionSolveUnresolved => "curve-expression.solve-unresolved",
            Self::CurveExpressionSolveControlUnresolved => {
                "curve-expression.solve-control-unresolved"
            }
            Self::CurveExpressionKindProhibited => "curve-expression.kind-prohibited",
            Self::PlaneSupportVariantsCapped => "geometry.plane-support-variants-capped",
            Self::SectionSignBranchesCapped => "section.sign-branches-capped",
            Self::CurveExpressionTextCapped => "curve-expression.text-capped",
            Self::TocScanCapped => "container.toc-scan-capped",
            Self::AssemblyComponentsExternal => "assembly.components-external",
            Self::AssemblyMemberTableIncomplete => "assembly.member-table-incomplete",
            Self::AssemblyPlacementUnresolved => "assembly.placement-unresolved",
            Self::AssemblyMemberReferenceUnresolved => "assembly.member-reference-unresolved",
            Self::AssemblyConstraintsNotTransferred => "assembly.constraints-not-transferred",
        }
    }

    /// The severity of this loss.
    #[must_use]
    const fn severity(self) -> Severity {
        match self {
            Self::ContainerCensus
            | Self::VisibGeomSurfaceAmbiguous
            | Self::VisibGeomCurveAmbiguous
            | Self::CarrierVisibGeomPlanes
            | Self::CarrierTopologyBoundPlanes
            | Self::CarrierFirstInstancePrototypes
            | Self::CarrierPairedEnvelopeSpheres
            | Self::CarrierSweepEnvelopeReplays
            | Self::CarrierSampledSweepCircles
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
            | Self::CarrierTorusParameterRetention
            | Self::AssemblyComponentsExternal => Severity::Info,
            Self::BrepTransferIncomplete
            | Self::BrepCompactFrameComponentUnresolved
            | Self::BrepCompactFrameBoundaryUnresolved
            | Self::GeometryInstanceCarriersGated
            | Self::TopologyIncompleteComponents
            | Self::TopologyUnownedComponent
            | Self::TopologyExplicitUnownedSurface
            | Self::TopologyVertexIdentifierUnstatable
            | Self::ExtrusionBodyRejected => Severity::Blocking,
            Self::PmiAssociationsUnresolved
            | Self::PmiTableIncomplete
            | Self::PmiRecordUnresolved
            | Self::PmiModifiersUnresolved
            | Self::PmiLengthUnitAssumed
            | Self::PmiAttributeUnitAssumed
            | Self::PmiTextMagnitudeInferred
            | Self::PmiDatumIdentityInferred
            | Self::PmiDatumLabelUnresolved
            | Self::PmiFirstAttributeAssumed
            | Self::PmiDatumTargetInferred
            | Self::PmiToleranceTargetInferred
            | Self::PmiDimensionTargetInferred
            | Self::PmiTargetNamespaceAmbiguous
            | Self::PmiDatumTargetUnresolved
            | Self::PmiDatumTagBoundaryAssumed
            | Self::PmiDatumDefinitionSlotAssumed
            | Self::PmiDimensionKindInferred
            | Self::PmiAngularUnitAssumed
            | Self::SectionSplineUnresolved
            | Self::CompressedSectionUnexpanded
            | Self::PhysicalGeometryOwnerUnresolved
            | Self::ReferenceConicPrefixUnresolved
            | Self::SurfacePrototypeFieldRetained
            | Self::SweepEnvelopeReplayUnresolved
            | Self::PcurveSampleCoordinatesDropped
            | Self::AngularPcurvePairUnresolved
            | Self::PlaneCompactFrameUnresolved
            | Self::PlaneAngularFrameUnresolved
            | Self::AngularEndpointLiteralUnresolved
            | Self::CylinderRotationEnvelopeUnresolved
            | Self::ActiveBoundaryPlaneUnresolved
            | Self::RevolutionCarrierUnresolved
            | Self::RevolutionBoundaryIntervalUnresolved
            | Self::SourceDialectUnverified
            | Self::LegacyRealValueUnresolved
            | Self::LegacyIntegerValueUnresolved
            | Self::LegacyContinuationFormUndefined
            | Self::LegacyByteStringEncodingRetained
            | Self::LegacyUnsignedValueUnresolved
            | Self::LegacyCompactRealUnresolved
            | Self::LegacyObjectArrayIncomplete
            | Self::LegacyObjectPayloadUndefined
            | Self::LegacyStringArrayIncomplete
            | Self::LegacyStringContinuationUndefined
            | Self::LegacyStringEncodingRetained
            | Self::TriangleStripRepresentationConflict
            | Self::VisibGeomSurfaceUntransferred
            | Self::PlaneLocalSystemUnresolved
            | Self::PlaneEnvelopeUnresolved
            | Self::PlaneCarrierUnresolved
            | Self::PrototypeLocalSystemUnresolved
            | Self::CylinderRotationFrameUnresolved
            | Self::TwoChartScalarFrameUnresolved
            | Self::PcurveScalarFrameUnresolved
            | Self::PcurveChartUnresolved
            | Self::SampledPcurveCarrierUnresolved
            | Self::CylinderIntersectionPcurveUnresolved
            | Self::CountFramedCurveCarrierUnverified
            | Self::HoleCircleIntervalUnresolved
            | Self::HoleGeneratorUnresolved
            | Self::SampledPcurveEvaluationUnresolved
            | Self::VisibGeomCurveUntransferred
            | Self::CrossSectionCurveBodyOpaque
            | Self::RoundCarrierOwnerUnresolved
            | Self::LegacySurfaceCarrierUnresolved
            | Self::NurbsBoundaryCarrierUnresolved
            | Self::SectionSegmentGeometryUnresolved
            | Self::FeatureNeutralSemanticsIncomplete
            | Self::FeatureSweepIncomplete
            | Self::FeatureSurfaceOperationIncomplete
            | Self::FeatureConstructionIncomplete
            | Self::FeatureRecognizedIncomplete
            | Self::FeatureNativeSemantics
            | Self::FeatureStateSelectionUnresolved
            | Self::FeatureConstructionUnresolved
            | Self::SectionSegmentMissing
            | Self::SectionRelationMissing
            | Self::SectionRelationTableMalformed
            | Self::SectionIncidenceMissing
            | Self::SectionRelationJoinMissing
            | Self::SectionIncidenceNative
            | Self::SectionRelationNative
            | Self::SectionVariableScalarOpaque
            | Self::SectionSolverVariableMissing
            | Self::SectionDimensionValueUnresolved
            | Self::SectionDiameterValueUnresolved
            | Self::ConfigurationDriverUnresolved
            | Self::CurveExpressionProhibited
            | Self::CurveExpressionSolveUnresolved
            | Self::CurveExpressionSolveControlUnresolved
            | Self::CurveExpressionKindProhibited
            | Self::PlaneSupportVariantsCapped
            | Self::SectionSignBranchesCapped
            | Self::CurveExpressionTextCapped
            | Self::TocScanCapped
            | Self::AssemblyMemberTableIncomplete
            | Self::AssemblyPlacementUnresolved
            | Self::AssemblyMemberReferenceUnresolved
            | Self::AssemblyConstraintsNotTransferred => Severity::Warning,
        }
    }

    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::SourceDialectUnverified => LossTaxonomy::SourceDialectUnverified,
            Self::ContainerCensus
            | Self::CarrierVisibGeomPlanes
            | Self::CarrierTopologyBoundPlanes
            | Self::CarrierFirstInstancePrototypes
            | Self::CarrierPairedEnvelopeSpheres
            | Self::CarrierSweepEnvelopeReplays
            | Self::CarrierSampledSweepCircles
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
            Self::PmiAssociationsUnresolved
            | Self::PmiTableIncomplete
            | Self::PmiRecordUnresolved
            | Self::PmiModifiersUnresolved
            | Self::PmiDatumLabelUnresolved
            | Self::PmiDatumTargetUnresolved
            | Self::PmiTargetNamespaceAmbiguous => LossTaxonomy::PmiOmitted,
            Self::PmiLengthUnitAssumed
            | Self::PmiAttributeUnitAssumed
            | Self::PmiTextMagnitudeInferred
            | Self::PmiDatumIdentityInferred
            | Self::PmiFirstAttributeAssumed
            | Self::PmiDatumTargetInferred
            | Self::PmiToleranceTargetInferred
            | Self::PmiDimensionTargetInferred
            | Self::PmiDatumTagBoundaryAssumed
            | Self::PmiDatumDefinitionSlotAssumed
            | Self::PmiDimensionKindInferred
            | Self::PmiAngularUnitAssumed => LossTaxonomy::DecodeDiagnostic,
            Self::LegacyRealValueUnresolved
            | Self::CompressedSectionUnexpanded
            | Self::PhysicalGeometryOwnerUnresolved
            | Self::ReferenceConicPrefixUnresolved
            | Self::TocScanCapped
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
            | Self::BrepCompactFrameComponentUnresolved
            | Self::BrepCompactFrameBoundaryUnresolved
            | Self::GeometryInstanceCarriersGated
            | Self::VisibGeomSurfaceUntransferred
            | Self::PlaneLocalSystemUnresolved
            | Self::PlaneEnvelopeUnresolved
            | Self::PlaneCarrierUnresolved
            | Self::PrototypeLocalSystemUnresolved
            | Self::CylinderRotationFrameUnresolved
            | Self::TwoChartScalarFrameUnresolved
            | Self::PcurveScalarFrameUnresolved
            | Self::PcurveChartUnresolved
            | Self::SampledPcurveCarrierUnresolved
            | Self::CylinderIntersectionPcurveUnresolved
            | Self::HoleCircleIntervalUnresolved
            | Self::HoleGeneratorUnresolved
            | Self::SampledPcurveEvaluationUnresolved
            | Self::VisibGeomCurveUntransferred
            | Self::CrossSectionCurveBodyOpaque
            | Self::RoundCarrierOwnerUnresolved
            | Self::LegacySurfaceCarrierUnresolved
            | Self::NurbsBoundaryCarrierUnresolved
            | Self::VisibGeomSurfaceAmbiguous
            | Self::VisibGeomCurveAmbiguous
            | Self::SectionSegmentGeometryUnresolved
            | Self::SectionSplineUnresolved
            | Self::SurfacePrototypeFieldRetained
            | Self::SweepEnvelopeReplayUnresolved
            | Self::PcurveSampleCoordinatesDropped
            | Self::AngularPcurvePairUnresolved
            | Self::PlaneCompactFrameUnresolved
            | Self::PlaneAngularFrameUnresolved
            | Self::AngularEndpointLiteralUnresolved
            | Self::CylinderRotationEnvelopeUnresolved
            | Self::ActiveBoundaryPlaneUnresolved
            | Self::RevolutionCarrierUnresolved
            | Self::RevolutionBoundaryIntervalUnresolved
            | Self::PlaneSupportVariantsCapped => LossTaxonomy::GeometryNotTransferred,
            Self::CountFramedCurveCarrierUnverified => LossTaxonomy::CarrierAxisInferred,
            Self::TopologyIncompleteComponents
            | Self::TopologyUnownedComponent
            | Self::TopologyExplicitUnownedSurface
            | Self::TopologyVertexIdentifierUnstatable
            | Self::ExtrusionBodyRejected => LossTaxonomy::TopologyNotTransferred,
            Self::FeatureNeutralSemanticsIncomplete
            | Self::FeatureSweepIncomplete
            | Self::FeatureSurfaceOperationIncomplete
            | Self::FeatureConstructionIncomplete
            | Self::FeatureRecognizedIncomplete
            | Self::FeatureNativeSemantics
            | Self::FeatureStateSelectionUnresolved
            | Self::FeatureConstructionUnresolved
            | Self::SectionSegmentMissing
            | Self::SectionRelationMissing
            | Self::SectionRelationTableMalformed
            | Self::SectionIncidenceMissing
            | Self::SectionRelationJoinMissing
            | Self::SectionIncidenceNative
            | Self::SectionRelationNative
            | Self::SectionVariableScalarOpaque
            | Self::SectionSolverVariableMissing
            | Self::SectionDimensionValueUnresolved
            | Self::SectionDiameterValueUnresolved
            | Self::ConfigurationDriverUnresolved
            | Self::CurveExpressionProhibited
            | Self::CurveExpressionSolveUnresolved
            | Self::CurveExpressionSolveControlUnresolved
            | Self::CurveExpressionKindProhibited
            | Self::SectionSignBranchesCapped
            | Self::CurveExpressionTextCapped => LossTaxonomy::FeatureHistoryRetained,
            Self::AssemblyComponentsExternal | Self::AssemblyMemberReferenceUnresolved => {
                LossTaxonomy::AssemblyComponentsExternal
            }
            Self::AssemblyMemberTableIncomplete
            | Self::AssemblyPlacementUnresolved
            | Self::AssemblyConstraintsNotTransferred => {
                LossTaxonomy::AssemblyPlacementsNotTransferred
            }
        }
    }

    /// Namespaced [`LossKind`] for this local code, classified by taxonomy.
    #[must_use]
    pub(crate) fn kind(self) -> LossKind {
        LossKind::namespaced(
            const {
                match cadmpeg_ir::report::loss::LossNamespace::new("creo") {
                    Ok(namespace) => namespace,
                    Err(_) => panic!("reserved codec namespace"),
                }
            },
            self.code(),
            self.shared_taxonomy(),
        )
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    ///
    /// The structured code is `creo/<local>`. Severity comes from the local
    /// code; the strict floor comes from the taxonomy.
    #[must_use]
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
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
                "container.census",
                "container.compressed-section-unexpanded",
                "container.physical-geometry-owner-unresolved",
                "reference.conic-prefix-unresolved",
                "source.dialect-unverified",
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
                "pmi.associations-unresolved",
                "pmi.table-incomplete",
                "pmi.record-unresolved",
                "pmi.modifiers-unresolved",
                "pmi.length-unit-assumed",
                "pmi.attribute-unit-assumed",
                "pmi.text-magnitude-inferred",
                "pmi.datum-identity-inferred",
                "pmi.datum-label-unresolved",
                "pmi.first-attribute-assumed",
                "pmi.datum-target-inferred",
                "pmi.tolerance-target-inferred",
                "pmi.dimension-target-inferred",
                "pmi.target-namespace-ambiguous",
                "pmi.datum-target-unresolved",
                "pmi.datum-tag-boundary-assumed",
                "pmi.datum-definition-slot-assumed",
                "pmi.dimension-kind-inferred",
                "pmi.angular-unit-assumed",
                "geometry.triangle-strip-conflict",
                "geometry.brep-incomplete",
                "geometry.brep-compact-frame-component-unresolved",
                "geometry.brep-compact-frame-boundary-unresolved",
                "geometry.section-spline-unresolved",
                "geometry.surface-prototype-field-retained",
                "geometry.plane-compact-frame-unresolved",
                "geometry.plane-angular-frame-unresolved",
                "geometry.angular-endpoint-literal-unresolved",
                "geometry.cylinder-rotation-envelope-unresolved",
                "geometry.active-boundary-plane-unresolved",
                "geometry.revolution-carrier-unresolved",
                "geometry.revolution-boundary-interval-unresolved",
                "topology.extrusion-body-rejected",
                "geometry.instance-carriers-gated",
                "geometry.visibgeom-surface-untransferred",
                "geometry.plane-local-system-unresolved",
                "geometry.plane-envelope-unresolved",
                "geometry.plane-carrier-unresolved",
                "geometry.prototype-local-system-unresolved",
                "geometry.cylinder-rotation-frame-unresolved",
                "geometry.two-chart-scalar-frame-unresolved",
                "geometry.pcurve-scalar-frame-unresolved",
                "geometry.pcurve-chart-unresolved",
                "geometry.sampled-pcurve-carrier-unresolved",
                "geometry.cylinder-intersection-pcurve-unresolved",
                "geometry.count-framed-curve-carrier-unverified",
                "geometry.hole-circle-interval-unresolved",
                "geometry.hole-generator-unresolved",
                "geometry.sweep-envelope-replay-unresolved",
                "geometry.pcurve-sample-coordinates-dropped",
                "geometry.angular-pcurve-pair-unresolved",
                "geometry.sampled-pcurve-evaluation-unresolved",
                "geometry.visibgeom-curve-untransferred",
                "curve.cross-section-body-opaque",
                "geometry.round-carrier-owner-unresolved",
                "geometry.legacy-surface-carrier-unresolved",
                "geometry.nurbs-boundary-carrier-unresolved",
                "geometry.visibgeom-surface-ambiguous",
                "geometry.visibgeom-curve-ambiguous",
                "geometry.section-segment-unresolved",
                "carrier.visibgeom-planes",
                "carrier.topology-bound-planes",
                "carrier.first-instance-prototypes",
                "carrier.paired-envelope-spheres",
                "carrier.positional-tori",
                "carrier.sweep-envelope-replays",
                "carrier.sampled-sweep-circles",
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
                "topology.unowned-component",
                "topology.explicit-unowned-surface",
                "topology.vertex-identifier-unstatable",
                "feature.neutral-semantics-incomplete",
                "feature.sweep-incomplete",
                "feature.surface-operation-incomplete",
                "feature.construction-incomplete",
                "feature.recognized-incomplete",
                "feature.native-semantics",
                "feature.state-selection-unresolved",
                "feature.construction-unresolved",
                "section.segment-missing",
                "section.relation-missing",
                "section.relation-table-malformed",
                "section.incidence-missing",
                "section.relation-join-missing",
                "section.incidence-native",
                "section.relation-native",
                "section.variable-scalar-opaque",
                "section.solver-variable-missing",
                "section.dimension-value-unresolved",
                "section.diameter-value-unresolved",
                "configuration.driver-unresolved",
                "curve-expression.prohibited",
                "curve-expression.solve-unresolved",
                "curve-expression.solve-control-unresolved",
                "curve-expression.kind-prohibited",
                "geometry.plane-support-variants-capped",
                "section.sign-branches-capped",
                "curve-expression.text-capped",
                "container.toc-scan-capped",
                "assembly.components-external",
                "assembly.member-table-incomplete",
                "assembly.placement-unresolved",
                "assembly.member-reference-unresolved",
                "assembly.constraints-not-transferred",
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
