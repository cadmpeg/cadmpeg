// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary charged by the sealed decode wrapper.
//!
//! These codes belong to no codec: the wrapper charges them by comparing the
//! content classes the requested depth names with the classes the document
//! holds, for a class a decoder states the source holds beyond the delivered
//! depth, for the records its normalizer gave up, and for an id a decoder
//! emitted twice with different content, so every codec reports them under
//! one namespace.

use cadmpeg_core::decode::ArenaClass;

use crate::report::{
    loss::{LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind},
    Severity,
};

const NAMESPACE: LossNamespace<'static> = crate::loss_namespace!("decode");

/// A stable, machine-readable identifier for one wrapper-charged decode loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeLossCode {
    /// The document lacks a content class the requested depth names, or
    /// one the source holds beyond the delivered depth.
    DepthNotDelivered {
        /// The class the document does not hold.
        class: ArenaClass,
    },
    /// A decoder emitted one id more than once in an arena, and the copies
    /// differ. The wrapper states no copy and gives the id up as a part.
    IdRepeated,
    /// The normalizer gave up records part way, which keep spellings the
    /// decoder stated.
    NormalizeIncomplete,
    /// A decoder dropped a record only because it needs a given-up record.
    /// The wrapper names the record in that give-up's `record.unreadable`
    /// and removes the note.
    RecordDroppedWith,
}

impl DecodeLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::DepthNotDelivered { .. } => "depth.not-delivered",
            Self::IdRepeated => "id.repeated",
            Self::NormalizeIncomplete => "normalize.incomplete",
            Self::RecordDroppedWith => "record.dropped-with",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::DepthNotDelivered { .. } => Severity::Error,
            Self::IdRepeated => Severity::Error,
            Self::NormalizeIncomplete => Severity::Warning,
            Self::RecordDroppedWith => Severity::Info,
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
            Self::IdRepeated | Self::NormalizeIncomplete | Self::RecordDroppedWith => {
                LossTaxonomy::DecodeDiagnostic
            }
        }
    }

    /// The strict floor §3.2 states: a given-up id refuses a pinned decode.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::IdRepeated => Some(Severity::Error),
            Self::DepthNotDelivered { .. }
            | Self::NormalizeIncomplete
            | Self::RecordDroppedWith => None,
        }
    }

    pub(crate) fn kind(self) -> LossKind {
        NamespacedLossKind::new(NAMESPACE, self.code(), self.shared_taxonomy())
            .with_strict_floor(self.strict_floor())
            .into()
    }

    /// Build a [`LossNote`] for this code with the given per-instance message.
    pub(crate) fn note(self, message: impl Into<String>) -> LossNote {
        LossNote::new(self.kind(), message).with_severity(self.severity())
    }
}
