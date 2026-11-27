// SPDX-License-Identifier: Apache-2.0
//! Stable Solid Edge transfer-loss vocabulary.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};

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
    PartitionUnreadable,
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
    LoopUnreadable,
    TopologyFaceBoundaryUnstated,
    FaceSenseUnresolved,
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
            Self::FeatureRollbackUnread => (
                "feature.rollback-unread",
                LossTaxonomy::FeatureHistoryRetained,
            ),
            Self::PartitionUnreadable => {
                ("partition.unreadable", LossTaxonomy::GeometryNotTransferred)
            }
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
            Self::LoopUnreadable => ("loop.unreadable", LossTaxonomy::TopologyNotTransferred),
            Self::TopologyFaceBoundaryUnstated => (
                "topology.face-boundary-unstated",
                LossTaxonomy::TopologyNotTransferred,
            ),
            Self::FaceSenseUnresolved => {
                ("face.sense-unresolved", LossTaxonomy::SourceTopologyInvalid)
            }
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
        LossNote::new(LossKind::namespaced(NAMESPACE, code, taxonomy), message)
    }
}
