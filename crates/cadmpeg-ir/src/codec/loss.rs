// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary charged by the sealed decode wrapper.
//!
//! These codes belong to no codec: the wrapper charges them by comparing the
//! content classes the requested depth names with the classes the document
//! holds, and for the records its normalizer gave up, so every codec reports
//! them under one namespace.

use cadmpeg_core::decode::ArenaClass;

use crate::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

const NAMESPACE: LossNamespace<'static> = crate::loss_namespace!("decode");

/// A stable, machine-readable identifier for one wrapper-charged decode loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeLossCode {
    /// The document lacks a content class the requested depth names.
    DepthNotDelivered {
        /// The requested class the document does not hold.
        class: ArenaClass,
    },
    /// The normalizer gave up records part way, which keep spellings the
    /// decoder stated.
    NormalizeIncomplete,
}

impl DecodeLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::DepthNotDelivered { .. } => "depth.not-delivered",
            Self::NormalizeIncomplete => "normalize.incomplete",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::DepthNotDelivered { .. } => Severity::Error,
            Self::NormalizeIncomplete => Severity::Warning,
        }
    }

    /// Shared category of the content the shallower depth left out.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::DepthNotDelivered {
                class: ArenaClass::Model,
            } => LossTaxonomy::GeometryNotTransferred,
            Self::DepthNotDelivered {
                class: ArenaClass::Design | ArenaClass::Process,
            } => LossTaxonomy::ParametricRecordOmitted,
            Self::NormalizeIncomplete => LossTaxonomy::DecodeDiagnostic,
        }
    }

    /// A shallower delivery refuses a strict decode. A record the
    /// normalizer gave up keeps all its content in a spelling the decoder
    /// stated, so it refuses nothing.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::DepthNotDelivered { .. } => Some(Severity::Warning),
            Self::NormalizeIncomplete => None,
        }
    }

    fn kind(self) -> LossKind {
        NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy())
            .with_strict_floor(self.strict_floor())
            .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}
