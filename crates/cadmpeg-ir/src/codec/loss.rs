// SPDX-License-Identifier: Apache-2.0
//! Shared decode loss vocabulary for artifact loading and the sealed wrapper.
//!
//! Fixed factories keep the namespace, code and severity consistent. The
//! wrapper compares requested classes with the scope a backend delivered.

use crate::document::ContentScope;

use crate::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

const NAMESPACE: LossNamespace<'static> = crate::loss_namespace!("decode");

/// A stable, machine-readable identifier for one shared decode loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeLossCode {
    /// One independently framed part could not be decoded.
    RecordUnreadable,
    /// A sidecar does not match the source document and was not consumed.
    SidecarStale,
    /// The document lacks a class required by the request.
    DepthNotDelivered {
        /// The scope held by the document.
        delivered: ContentScope,
    },
}

impl DecodeLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::DepthNotDelivered { .. } => "depth.not-delivered",
            Self::RecordUnreadable => "record.unreadable",
            Self::SidecarStale => "sidecar.stale",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::DepthNotDelivered { .. } | Self::RecordUnreadable => Severity::Error,
            Self::SidecarStale => Severity::Warning,
        }
    }

    /// Shared category of the unavailable content.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::DepthNotDelivered { delivered } if delivered.model => {
                LossTaxonomy::ParametricRecordOmitted
            }
            Self::DepthNotDelivered { .. } => LossTaxonomy::GeometryNotTransferred,
            Self::SidecarStale => LossTaxonomy::PreservedSourceUnavailable,
            Self::RecordUnreadable => LossTaxonomy::DecodeDiagnostic,
        }
    }

    fn kind(self) -> LossKind {
        let kind = NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy());
        match self {
            Self::DepthNotDelivered { .. } => kind.with_strict_floor(Some(Severity::Warning)),
            Self::RecordUnreadable => kind.with_strict_floor(Some(Severity::Error)),
            Self::SidecarStale => kind,
        }
        .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    pub fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}

#[cfg(test)]
mod tests;
