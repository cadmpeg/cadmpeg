// SPDX-License-Identifier: Apache-2.0
//! Stable Solid Edge transfer-loss vocabulary.

use cadmpeg_ir::report::loss::{LossKind, LossNamespace, LossNote, LossTaxonomy};

const NAMESPACE: LossNamespace<'static> = match LossNamespace::new("solidedge") {
    Ok(value) => value,
    Err(_) => panic!("invalid loss namespace"),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolidEdgeLossCode {
    ContainerNoncanonical,
    BodiesPending,
}

impl SolidEdgeLossCode {
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        let (code, taxonomy) = match self {
            Self::ContainerNoncanonical => (
                "container.noncanonical",
                LossTaxonomy::NoncanonicalSourceSyntax,
            ),
            Self::BodiesPending => ("body.decode-pending", LossTaxonomy::GeometryNotTransferred),
        };
        LossNote::new(LossKind::namespaced(NAMESPACE, code, taxonomy), message)
    }
}
