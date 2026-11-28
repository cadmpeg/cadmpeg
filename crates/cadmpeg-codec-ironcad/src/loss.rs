// SPDX-License-Identifier: Apache-2.0
//! Stable `IronCAD` transfer-loss vocabulary.

use cadmpeg_ir::report::loss::{LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind};
use cadmpeg_ir::report::Severity;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("ironcad");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IronCadLossCode {
    ContainerNoncanonical,
    FrameUnreadable,
    FrameTrailingBytes,
    RecordHeaderUnrecognized,
    KernelUnrecognized,
    KernelPrologueUnreadable,
    KernelUnverified,
    ParasolidBodyPartial,
    BodyRecordsRetained,
    BodyExtensionsRetained,
    BodyPartitionUnread,
    FeatureShapeRetained,
    FaceBoundaryUnstated,
    SurfaceUntyped,
    PlacementBoundByOrder,
    PlacementUnbound,
    PlacementMatrixSearched,
    PlacementRecordVersionUnrecognized,
    AssemblyFlattened,
    AssemblyBoundByOrder,
    PropertyUnread,
    UnitsUnrecognized,
    UnitsScaleConflict,
    SceneStreamsPending,
}

impl IronCadLossCode {
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        let (code, taxonomy) = match self {
            Self::ContainerNoncanonical => (
                "container.noncanonical",
                LossTaxonomy::NoncanonicalSourceSyntax,
            ),
            Self::FrameUnreadable => ("frame.unreadable", LossTaxonomy::MissingGeometryStream),
            Self::FrameTrailingBytes => ("frame.trailing-bytes", LossTaxonomy::DecodeDiagnostic),
            Self::RecordHeaderUnrecognized => {
                ("record.header-unrecognized", LossTaxonomy::DecodeDiagnostic)
            }
            Self::KernelUnrecognized => (
                "record.kernel-unrecognized",
                LossTaxonomy::GeometryNotTransferred,
            ),
            Self::KernelPrologueUnreadable => (
                "record.prologue-unreadable",
                LossTaxonomy::SourceDialectUnverified,
            ),
            Self::KernelUnverified => ("kernel.unverified", LossTaxonomy::SourceDialectUnverified),
            Self::ParasolidBodyPartial => (
                "body.parasolid-partial",
                LossTaxonomy::GeometryNotTransferred,
            ),
            Self::BodyRecordsRetained => ("body.records-retained", LossTaxonomy::RecordNotTyped),
            Self::BodyExtensionsRetained => (
                "body.extensions-retained",
                LossTaxonomy::GeometryNotTransferred,
            ),
            Self::BodyPartitionUnread => (
                "body.partition-unread",
                LossTaxonomy::FeatureHistoryRetained,
            ),
            Self::FeatureShapeRetained => (
                "body.feature-shape-retained",
                LossTaxonomy::FeatureHistoryRetained,
            ),
            Self::FaceBoundaryUnstated => (
                "face.boundary-unstated",
                LossTaxonomy::TopologyNotTransferred,
            ),
            Self::SurfaceUntyped => ("surface.untyped", LossTaxonomy::GeometryNotTransferred),
            Self::PlacementBoundByOrder => {
                ("placement.bound-by-order", LossTaxonomy::DecodeDiagnostic)
            }
            Self::PlacementUnbound => (
                "placement.unbound",
                LossTaxonomy::AssemblyPlacementsNotTransferred,
            ),
            Self::PlacementMatrixSearched => {
                ("placement.matrix-searched", LossTaxonomy::DecodeDiagnostic)
            }
            Self::PlacementRecordVersionUnrecognized => (
                "placement.record-version-unrecognized",
                LossTaxonomy::DecodeDiagnostic,
            ),
            Self::AssemblyFlattened => (
                "assembly.flattened",
                LossTaxonomy::AssemblyPlacementsNotTransferred,
            ),
            Self::UnitsUnrecognized => ("units.unrecognized", LossTaxonomy::MetadataNotTransferred),
            Self::UnitsScaleConflict => ("units.scale-conflict", LossTaxonomy::DecodeDiagnostic),
            Self::PropertyUnread => ("property.unread", LossTaxonomy::MetadataNotTransferred),
            Self::AssemblyBoundByOrder => {
                ("assembly.bound-by-order", LossTaxonomy::DecodeDiagnostic)
            }
            Self::SceneStreamsPending => (
                "scene.decode-pending",
                LossTaxonomy::ObjectRecordsUntransferred,
            ),
        };
        LossNote::new(
            NamespacedLossKind::new(NAMESPACE, code, taxonomy)
                .with_strict_floor(self.strict_floor()),
            message,
        )
    }

    /// The strict floor `ironcad.md` (Strict floors) states: a compound-file
    /// deviation that the reader recovers from pins Warning.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::ContainerNoncanonical => Some(Severity::Warning),
            _ => None,
        }
    }
}
