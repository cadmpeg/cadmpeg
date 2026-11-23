// SPDX-License-Identifier: Apache-2.0
//! Stable Solid Edge transfer-loss vocabulary.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("solidedge");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolidEdgeLossCode {
    ContainerNoncanonical,
    BodiesPending,
    PartitionUnreadable,
    PartitionIncomplete,
    PrimaryStateAssumed,
    SurfaceUnsupported,
    CurveUnsupported,
    EdgeTrimUnresolved,
    KernelUnverified,
    FeaturesPending,
    AttributesPending,
    WireTopologyUnsupported,
    BodyKindAssumed,
    IntersectionFrameFromChart,
    IntersectionChartReduced,
    EdgeCarrierSampled,
    IntersectionFrameFromSupportsAssumed,
    TopologyToleranceInvalid,
    BodyResolutionInvalid,
    IntersectionPlanarSupportAssumed,
    IntersectionNurbsBoundaryAssumed,
    IntersectionNurbsIsolineAssumed,
}

impl SolidEdgeLossCode {
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        let (code, taxonomy) = match self {
            Self::ContainerNoncanonical => (
                "container.noncanonical",
                LossTaxonomy::NoncanonicalSourceSyntax,
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
            Self::TopologyToleranceInvalid => (
                "topology.tolerance-invalid",
                LossTaxonomy::SourceTopologyInvalid,
            ),
            Self::IntersectionFrameFromSupportsAssumed => (
                "curve.intersection-frame-from-supports-assumed",
                LossTaxonomy::CarrierAxisInferred,
            ),
            Self::EdgeCarrierSampled => (
                "curve.edge-carrier-sampled",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::IntersectionChartReduced => (
                "curve.intersection-chart-reduced",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::IntersectionFrameFromChart => (
                "curve.intersection-frame-from-chart",
                LossTaxonomy::CarrierAxisInferred,
            ),
            Self::BodyKindAssumed => ("body.kind-assumed", LossTaxonomy::TopologyGaugeSubstituted),
            Self::WireTopologyUnsupported => (
                "topology.wire-unsupported",
                LossTaxonomy::TopologyNotTransferred,
            ),
            Self::FeaturesPending => (
                "feature.decode-pending",
                LossTaxonomy::MetadataNotTransferred,
            ),
            Self::BodiesPending => ("body.decode-pending", LossTaxonomy::MissingGeometryStream),
        };
        LossNote::new(LossKind::namespaced(NAMESPACE, code, taxonomy), message)
    }
}
