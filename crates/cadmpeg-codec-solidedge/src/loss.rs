// SPDX-License-Identifier: Apache-2.0
//! Stable Solid Edge transfer-loss vocabulary.

use cadmpeg_ir::report::loss::{LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind};
use cadmpeg_ir::report::Severity;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("solidedge");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolidEdgeLossCode {
    ContainerNoncanonical,
    BodiesPending,
    UnitsUnread,
    UnitCodeUnknown,
    UnitNotNeutral,
    FeatureStorageUnread,
    FeatureNavigatorAbsent,
    FeatureOperandsPending,
    FeatureKindUnknown,
    FeatureRollbackUnread,
    FeatureFolderUnread,
    SketchUnread,
    SketchRelationsUnread,
    FeatureExtentUnread,
    FeatureDressupUnread,
    FeatureThreadUnread,
    FeatureSelectionUnread,
    FeaturePatternUnread,
    FamilyMemberUnread,
    AssemblyOccurrenceUnread,
    AssemblyReferenceUnread,
    AssemblyVisibilityUnread,
    SheetMetalRuleUnread,
    SheetMetalBendMethodUnread,
    SheetMetalBendValueUnstated,
    SheetMetalBaseTabUnread,
    SheetMetalFlangeFieldUnread,
    PartitionIncomplete,
    PrimaryStateAssumed,
    SurfaceUnsupported,
    CurveUnsupported,
    EdgeTrimUnresolved,
    KernelUnverified,
    AttributesPending,
    WireTopologyUnsupported,
    BodyKindAssumed,
    IntersectionChartReduced,
    EdgeCarrierSampled,
    IntersectionFrameFromSupportsAssumed,
    TopologyToleranceInvalid,
    BodyResolutionInvalid,
    BodyResolutionUnread,
    IntersectionPlanarSupportAssumed,
    IntersectionNurbsBoundaryAssumed,
    IntersectionNurbsIsolineAssumed,
    EdgeToleranceWidened,
    SurfaceUnresolved,
    CurveUnresolved,
    TopologyFaceBoundaryUnstated,
    ChainTruncated,
    VertexUnresolved,
    CurveConstructionDropped,
    RegionFacesUntransferred,
    BodyPlacementUnread,
    BodyConstruction,
}

impl SolidEdgeLossCode {
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        let (code, taxonomy) = match self {
            Self::ContainerNoncanonical => (
                "container.noncanonical",
                LossTaxonomy::NoncanonicalSourceSyntax,
            ),
            Self::FamilyMemberUnread => ("family.member-unread", LossTaxonomy::DecodeDiagnostic),
            Self::UnitsUnread => ("units.unread", LossTaxonomy::MetadataNotTransferred),
            Self::UnitCodeUnknown => ("units.code-unknown", LossTaxonomy::MetadataNotTransferred),
            Self::UnitNotNeutral => ("units.not-neutral", LossTaxonomy::MetadataNotTransferred),
            Self::FeatureStorageUnread => {
                ("feature.storage-unread", LossTaxonomy::DecodeDiagnostic)
            }
            Self::FeatureNavigatorAbsent => {
                ("feature.navigator-absent", LossTaxonomy::DecodeDiagnostic)
            }
            Self::FeatureOperandsPending => {
                ("feature.operands-pending", LossTaxonomy::RecordNotTyped)
            }
            Self::FeatureKindUnknown => ("feature.kind-unknown", LossTaxonomy::RecordNotTyped),
            Self::FeatureFolderUnread => (
                "feature.folder-unread",
                LossTaxonomy::FeatureHistoryRetained,
            ),
            Self::SketchUnread => ("sketch.unread", LossTaxonomy::FeatureHistoryRetained),
            Self::SketchRelationsUnread => {
                ("sketch.relations-unread", LossTaxonomy::RecordNotTyped)
            }
            Self::FeatureExtentUnread => ("feature.extent-unread", LossTaxonomy::RecordNotTyped),
            Self::FeatureDressupUnread => ("feature.dressup-unread", LossTaxonomy::RecordNotTyped),
            Self::FeatureThreadUnread => ("feature.thread-unread", LossTaxonomy::RecordNotTyped),
            Self::FeatureSelectionUnread => {
                ("feature.selection-unread", LossTaxonomy::RecordNotTyped)
            }
            Self::FeaturePatternUnread => ("feature.pattern-unread", LossTaxonomy::RecordNotTyped),
            Self::AssemblyOccurrenceUnread => {
                ("assembly.occurrence-unread", LossTaxonomy::DecodeDiagnostic)
            }
            Self::AssemblyReferenceUnread => {
                ("assembly.reference-unread", LossTaxonomy::DecodeDiagnostic)
            }
            Self::AssemblyVisibilityUnread => (
                "assembly.visibility-unread",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::SheetMetalRuleUnread => ("sheet-metal.rule-unread", LossTaxonomy::RecordNotTyped),
            Self::SheetMetalBendMethodUnread => (
                "sheet-metal.bend-method-unread",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::SheetMetalBendValueUnstated => (
                "sheet-metal.bend-value-unstated",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::SheetMetalBaseTabUnread => {
                ("sheet-metal.base-tab-unread", LossTaxonomy::RecordNotTyped)
            }
            Self::SheetMetalFlangeFieldUnread => (
                "sheet-metal.flange-field-unread",
                LossTaxonomy::RecordNotTyped,
            ),
            Self::FeatureRollbackUnread => (
                "feature.rollback-unread",
                LossTaxonomy::FeatureHistoryRetained,
            ),
            Self::PrimaryStateAssumed => (
                "partition.primary-state-assumed",
                LossTaxonomy::DecodeDiagnostic,
            ),
            Self::PartitionIncomplete => {
                ("partition.incomplete", LossTaxonomy::GeometryNotTransferred)
            }
            Self::SurfaceUnsupported => {
                ("surface.unsupported", LossTaxonomy::GeometryNotTransferred)
            }
            Self::CurveUnsupported => ("curve.unsupported", LossTaxonomy::GeometryNotTransferred),
            Self::EdgeTrimUnresolved => {
                ("edge.trim-unresolved", LossTaxonomy::GeometryNotTransferred)
            }
            Self::KernelUnverified => ("kernel.unverified", LossTaxonomy::SourceDialectUnverified),
            Self::AttributesPending => (
                "attributes.decode-pending",
                LossTaxonomy::AttributesNotTransferred,
            ),
            Self::IntersectionPlanarSupportAssumed => (
                "curve.intersection-planar-support-assumed",
                LossTaxonomy::CarrierAxisInferred,
            ),
            Self::IntersectionNurbsBoundaryAssumed => (
                "curve.intersection-nurbs-boundary-assumed",
                LossTaxonomy::CarrierAxisInferred,
            ),
            Self::IntersectionNurbsIsolineAssumed => (
                "curve.intersection-nurbs-isoline-assumed",
                LossTaxonomy::CarrierAxisInferred,
            ),
            Self::BodyResolutionInvalid => (
                "body.resolution-invalid",
                LossTaxonomy::SourceTopologyInvalid,
            ),
            Self::BodyResolutionUnread => {
                ("body.resolution-unread", LossTaxonomy::DecodeDiagnostic)
            }
            Self::TopologyToleranceInvalid => (
                "topology.tolerance-invalid",
                LossTaxonomy::SourceTopologyInvalid,
            ),
            Self::IntersectionFrameFromSupportsAssumed => (
                "curve.intersection-frame-from-supports-assumed",
                LossTaxonomy::CarrierAxisInferred,
            ),
            Self::EdgeToleranceWidened => (
                "edge.tolerance-widened",
                LossTaxonomy::GeometryAgreementSampled,
            ),
            Self::EdgeCarrierSampled => (
                "curve.edge-carrier-sampled",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::IntersectionChartReduced => (
                "curve.intersection-chart-reduced",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::BodyKindAssumed => ("body.kind-assumed", LossTaxonomy::TopologyGaugeSubstituted),
            Self::WireTopologyUnsupported => (
                "topology.wire-unsupported",
                LossTaxonomy::TopologyNotTransferred,
            ),
            Self::SurfaceUnresolved => ("surface.unresolved", LossTaxonomy::GeometryNotTransferred),
            Self::CurveUnresolved => ("curve.unresolved", LossTaxonomy::GeometryNotTransferred),
            Self::TopologyFaceBoundaryUnstated => (
                "topology.face-boundary-unstated",
                LossTaxonomy::TopologyNotTransferred,
            ),
            Self::ChainTruncated => (
                "topology.chain-truncated",
                LossTaxonomy::TopologyNotTransferred,
            ),
            Self::VertexUnresolved => ("vertex.unresolved", LossTaxonomy::SourceTopologyInvalid),
            Self::CurveConstructionDropped => (
                "curve.construction-dropped",
                LossTaxonomy::GeometryNotTransferred,
            ),
            Self::RegionFacesUntransferred => (
                "region.faces-untransferred",
                LossTaxonomy::TopologyNotTransferred,
            ),
            Self::BodyPlacementUnread => (
                "body.placement-unread",
                LossTaxonomy::AssemblyPlacementsNotTransferred,
            ),
            Self::BodyConstruction => ("body.construction", LossTaxonomy::MetadataNotTransferred),
            Self::BodiesPending => ("body.decode-pending", LossTaxonomy::MissingGeometryStream),
        };
        LossNote::new(
            NamespacedLossKind::new(NAMESPACE, code, taxonomy)
                .with_strict_floor(self.strict_floor()),
            message,
        )
    }

    /// The strict floor `solidedge.md` §1 states: a compound-file deviation
    /// that the reader recovers from pins Warning.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::ContainerNoncanonical => Some(Severity::Warning),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SolidEdgeLossCode;
    use cadmpeg_ir::report::Severity;

    #[test]
    fn the_compound_deviation_alone_pins_a_strict_floor() {
        assert_eq!(
            SolidEdgeLossCode::ContainerNoncanonical
                .note("")
                .code
                .strict_floor(),
            Some(Severity::Warning)
        );
        for code in [
            SolidEdgeLossCode::BodiesPending,
            SolidEdgeLossCode::PartitionIncomplete,
            SolidEdgeLossCode::ChainTruncated,
            SolidEdgeLossCode::CurveUnsupported,
        ] {
            assert_eq!(code.note("").code.strict_floor(), None, "{code:?}");
        }
    }
}
