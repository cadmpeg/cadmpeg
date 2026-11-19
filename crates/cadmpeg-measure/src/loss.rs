// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary charged by a measurement pass.

use cadmpeg_ir::report::loss::{LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind};
use cadmpeg_ir::report::Severity;

const NAMESPACE: LossNamespace<'static> = cadmpeg_ir::loss_namespace!("measure");

/// A stable, machine-readable identifier for one measurement loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeasureLossCode {
    /// The pass measured no body because the document's scope holds no
    /// `model` class.
    OutputEmpty,
}

impl MeasureLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::OutputEmpty => "output.empty",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::OutputEmpty => Severity::Warning,
        }
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        let kind =
            NamespacedLossKind::new(NAMESPACE, self.code(), LossTaxonomy::GeometryNotTransferred)
                .with_strict_floor(None);
        LossNote::new(kind, message).with_severity(self.severity())
    }
}
