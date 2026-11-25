// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary charged by the sealed decode wrapper.
//!
//! These codes belong to no codec: the wrapper charges them by comparing the
//! content classes the requested depth names with the classes the document
//! holds, for a class a decoder states the source holds beyond the delivered
//! depth, for the records its normalizer gave up, for an id a decoder
//! emitted twice with different content, for a document it took no
//! content digest of, and for the vertex tolerances it widened, so every
//! codec reports them under one namespace.

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
    /// differ. The first copy it emitted is kept and the others dropped
    /// (interim: §2.8 states no copy, which needs the shared give-up pass).
    IdRepeated,
    /// The normalizer gave up records part way, which keep spellings the
    /// decoder stated.
    NormalizeIncomplete,
    /// The document has no canonical JSON, so the wrapper took no content
    /// digest of it.
    ContentDigestUnavailable,
    /// The wrapper stated the tolerance of vertices the source stores
    /// farther from a compared curve end than the coincidence allowance.
    VertexToleranceWidened,
}

impl DecodeLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::DepthNotDelivered { .. } => "depth.not-delivered",
            Self::IdRepeated => "id.repeated",
            Self::NormalizeIncomplete => "normalize.incomplete",
            Self::ContentDigestUnavailable => "content-digest.unavailable",
            Self::VertexToleranceWidened => "geometry.vertex-tolerance-widened",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::DepthNotDelivered { .. } => Severity::Error,
            Self::IdRepeated => Severity::Error,
            Self::NormalizeIncomplete | Self::ContentDigestUnavailable => Severity::Warning,
            Self::VertexToleranceWidened => Severity::Info,
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
            Self::IdRepeated
            | Self::NormalizeIncomplete
            | Self::ContentDigestUnavailable
            | Self::VertexToleranceWidened => LossTaxonomy::DecodeDiagnostic,
        }
    }

    /// A shallower delivery refuses a strict decode. A record the
    /// normalizer gave up keeps all its content in a spelling the decoder
    /// stated, so it refuses nothing; a repeated id, a missing content
    /// digest, and a widened tolerance refuse nothing either.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::DepthNotDelivered { .. } => Some(Severity::Warning),
            Self::IdRepeated
            | Self::NormalizeIncomplete
            | Self::ContentDigestUnavailable
            | Self::VertexToleranceWidened => None,
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
