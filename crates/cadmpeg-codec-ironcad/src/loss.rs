// SPDX-License-Identifier: Apache-2.0
//! Stable `IronCAD` transfer-loss vocabulary.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("ironcad");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IronCadLossCode {
    ContainerNoncanonical,
    FrameUnreadable,
    RecordTreeUnreadable,
    RecordHeaderUnrecognized,
    KernelUnrecognized,
    KernelPrologueUnreadable,
    KernelUnverified,
    BodiesPending,
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
            Self::RecordTreeUnreadable => (
                "record-tree.unreadable",
                LossTaxonomy::GeometryNotTransferred,
            ),
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
            Self::BodiesPending => ("body.decode-pending", LossTaxonomy::GeometryNotTransferred),
            Self::SceneStreamsPending => (
                "scene.decode-pending",
                LossTaxonomy::ObjectRecordsUntransferred,
            ),
        };
        LossNote::new(LossKind::namespaced(NAMESPACE, code, taxonomy), message)
    }
}
