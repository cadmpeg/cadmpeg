// SPDX-License-Identifier: Apache-2.0
//! Stable loss vocabulary charged by the sealed decode wrapper.
//!
//! These codes belong to no codec: the wrapper charges them by comparing the
//! content classes the requested depth names with the classes the document
//! holds, for a class a decoder states the source holds beyond the delivered
//! depth, for the records its normalizer gave up, for an id a decoder
//! emitted twice with different content, for the vertex tolerances it
//! widened or could not measure, and for what the session's XML parses left
//! unread from a DOCTYPE, so every codec reports them under one namespace.

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
    /// The wrapper names the record in that give-up's loss and removes the
    /// note.
    RecordDroppedWith,
    /// The wrapper stated the tolerance of vertices the source stores
    /// farther from a compared position than the coincidence allowance.
    VertexToleranceWidened,
    /// The wrapper's vertex tolerance pass met a resource ceiling of its own
    /// session and widened no vertex tolerance in the geometry or resolved
    /// states it had not measured, or still widened a geometry at its bound
    /// of measures.
    VertexToleranceUnmeasured,
    /// An XML text's DOCTYPE names an external subset, whose declarations
    /// the decoder does not read.
    XmlExternalSubsetUnread,
    /// An XML text's internal subset declares attribute defaults, which the
    /// decoder does not apply.
    XmlAttributeDefaultsUnapplied,
    /// XML entity references with an unread declaration or external
    /// replacement expand to empty text.
    XmlExternalEntityUnexpanded,
}

impl DecodeLossCode {
    /// The stable string identifier. This is the gating contract.
    const fn code(self) -> &'static str {
        match self {
            Self::DepthNotDelivered { .. } => "depth.not-delivered",
            Self::IdRepeated => "id.repeated",
            Self::NormalizeIncomplete => "normalize.incomplete",
            Self::RecordDroppedWith => "record.dropped-with",
            Self::VertexToleranceWidened => "geometry.vertex-tolerance-widened",
            Self::VertexToleranceUnmeasured => "geometry.vertex-tolerance-unmeasured",
            Self::XmlExternalSubsetUnread => "xml.external-subset-unread",
            Self::XmlAttributeDefaultsUnapplied => "xml.attribute-defaults-unapplied",
            Self::XmlExternalEntityUnexpanded => "xml.external-entity-unexpanded",
        }
    }

    const fn severity(self) -> Severity {
        match self {
            Self::DepthNotDelivered { .. } => Severity::Error,
            Self::IdRepeated => Severity::Error,
            Self::NormalizeIncomplete => Severity::Warning,
            Self::RecordDroppedWith => Severity::Info,
            Self::VertexToleranceWidened => Severity::Info,
            Self::VertexToleranceUnmeasured => Severity::Warning,
            Self::XmlExternalSubsetUnread => Severity::Info,
            Self::XmlAttributeDefaultsUnapplied | Self::XmlExternalEntityUnexpanded => {
                Severity::Warning
            }
        }
    }

    /// Shared category of the content the shallower depth left out.
    const fn shared_taxonomy(self) -> LossTaxonomy {
        match self {
            Self::DepthNotDelivered {
                class: ArenaClass::Model,
            } => LossTaxonomy::GeometryNotTransferred,
            Self::DepthNotDelivered {
                class: ArenaClass::Design,
            } => LossTaxonomy::ParametricRecordOmitted,
            Self::IdRepeated
            | Self::NormalizeIncomplete
            | Self::RecordDroppedWith
            | Self::VertexToleranceWidened
            | Self::VertexToleranceUnmeasured
            | Self::XmlExternalSubsetUnread
            | Self::XmlAttributeDefaultsUnapplied
            | Self::XmlExternalEntityUnexpanded => LossTaxonomy::DecodeDiagnostic,
        }
    }

    /// The strict floor §3.2 states: a given-up id refuses a pinned decode.
    const fn strict_floor(self) -> Option<Severity> {
        match self {
            Self::IdRepeated => Some(Severity::Error),
            Self::DepthNotDelivered { .. }
            | Self::NormalizeIncomplete
            | Self::RecordDroppedWith
            | Self::VertexToleranceWidened
            | Self::VertexToleranceUnmeasured
            | Self::XmlExternalSubsetUnread
            | Self::XmlAttributeDefaultsUnapplied
            | Self::XmlExternalEntityUnexpanded => None,
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
