// SPDX-License-Identifier: Apache-2.0
//! Creo dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, [`DialectId::pinned`]
//! strings are the boundary, [`classify`] is the one construction
//! path, and the vocabulary is closed. Every layout has a row in
//! `docs/dialects.toml`; `tests::every_pinned_id_has_a_registry_row_and_every_row_has_a_variant`
//! fails on drift in either direction.
//!
//! # The discriminant is the layout classification
//!
//! Creo declares no version discriminant that partitions anything. `#UGC:2` is
//! the crate's format gate, not a dialect boundary: every admitted file carries
//! it. What does partition the document space is the persistence layout, which
//! `container::identify_layout` reads from the enumerated section
//! table before any decode strategy is chosen — a B1 grammar boundary. So
//! [`Layout`] is the identity vocabulary itself: [`Layout::id`] pins one
//! registry id per layout family, and the match is exhaustive so a new layout
//! cannot be added without pinning an id for it.
//!
//! [`Layout::token`] is a separate vocabulary and stays that way. It is the
//! value of the long-standing `layout` source attribute and of the inspect
//! note, and those strings are a contract of their own; the registry ids are a
//! contract of the identity registry. Merging them would couple two
//! independently pinned vocabularies for no gain.
//!
//! # `creo:unknown` is admitted, never refused
//!
//! [`Layout::Unknown`] is an error nowhere in this codec. Every
//! layout-conditional decode path is a positive gate on a named layout, so an
//! unclassified document simply runs the layout-independent path and skips all
//! of them. That is a real recovery strategy and it is charged as one:
//! [`Admission::AdmittedUnverified`] plus
//! [`CreoLossCode::SourceDialectUnverified`], both derived from
//! [`layout_is_declared`] so they cannot disagree.

use crate::container::{ContainerScan, Layout};
use crate::loss::CreoLossCode;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "creo";

/// Key of the `#UGC:2` header line, verbatim, in [`DialectMatch::declared`].
///
/// The first line of the file as the producer wrote it. It carries the
/// container magic and a producer token; it is evidence, and this codec
/// branches on none of it beyond the magic that `detect` already tested.
const DECLARED_VERSION_LINE: &str = "version_line";
/// Key of the legacy ASCII persistence-schema token in
/// [`DialectMatch::declared`]. Absent outside the legacy ASCII layout.
///
/// The decimal token following `#P_OBJECT`. Present exactly when a complete
/// legacy ASCII frame was validated, because the container discards a parsed
/// framing whenever the layout is not [`Layout::LegacyAscii`].
const DECLARED_LEGACY_ASCII_SCHEMA: &str = "legacy_ascii_schema";
/// Key of the legacy ASCII product-release token in
/// [`DialectMatch::declared`]. Absent when the `#Pro/ENGINEER` banner carries
/// no `Version` or `Release` word.
const DECLARED_LEGACY_ASCII_PRODUCT_RELEASE: &str = "legacy_ascii_product_release";

/// Whether the layout classification named a dialect whose own declared decode
/// strategy this run then applied.
///
/// The single predicate behind two facts that must never disagree: the
/// [`Admission`] in [`classify`] and the
/// [`CreoLossCode::SourceDialectUnverified`] charge in [`dialect_loss`]. Both
/// call this; neither recomputes it, so the biconditional the decode policy
/// requires holds by construction rather than by two authors agreeing.
///
/// True when a layout discriminant matched and the decode gates for that
/// layout ran — the only state that charges no loss. False when no discriminant
/// matched, so the decode ran the layout-independent path and skipped every
/// layout gate.
pub(crate) const fn layout_is_declared(layout: Layout) -> bool {
    match layout {
        Layout::Nd | Layout::Depdb | Layout::LegacyAscii => true,
        Layout::Unknown => false,
    }
}

/// The loss charged when no layout discriminant matched.
///
/// `None` exactly when [`layout_is_declared`] holds, which is also exactly
/// when [`classify`] reports [`Admission::Admitted`].
pub(crate) fn dialect_loss(layout: Layout) -> Option<LossNote> {
    if layout_is_declared(layout) {
        return None;
    }
    Some(CreoLossCode::SourceDialectUnverified.note(
        "the PSB section table carries no layout discriminant: no DEPDB_DATA section with the \
         p_dep_db root record, no ND: section-name decoration, and no complete legacy ASCII \
         #P_OBJECT frame. This decode ran the layout-independent path only, so every ND, DEPDB, \
         and legacy ASCII decode gate was skipped",
    ))
}

impl Layout {
    /// Every dialect this codec can name.
    ///
    /// The registry cross-check is its only consumer, and that is the point:
    /// the list exists so a layout added without a registry row, or a row
    /// added without a layout, fails a test.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] = [Self::Nd, Self::Depdb, Self::LegacyAscii, Self::Unknown];

    /// The pinned registry id.
    ///
    /// One row of `docs/dialects.toml` under the `creo` namespace, and the only
    /// registry string boundary this enum has. `docs/dialects.toml` declares
    /// `complete = false` for this format: the rows are the grammar classes this
    /// codec branches on plus the mandatory [`Layout::Unknown`] totality row
    /// (design §3.3, B4), not an enumeration of anything PTC publishes.
    ///
    /// Total by construction: [`Layout`] is closed and this match is
    /// exhaustive, so `detect`'s whole domain classifies.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::Nd => "creo:nd",
            Self::Depdb => "creo:depdb",
            Self::LegacyAscii => "creo:legacy-ascii",
            Self::Unknown => "creo:unknown",
        })
    }
}

/// Classifies one container scan. The single construction path for a
/// [`DialectMatch`] in this codec, so a classification bug and the report
/// can never disagree.
///
/// # `nearest` on the unclassified path
///
/// [`Admission::AdmittedUnverified`] documents `nearest` as the dialect
/// whose declared strategy was substituted for the parse. Creo's
/// unclassified path substitutes nothing — it skips every layout gate — so
/// the only row that describes the strategy actually applied is
/// `creo:unknown` itself. Naming `creo:nd` or `creo:depdb` would assert a
/// substitution that did not happen, which is the one thing the field must
/// not do.
pub(crate) fn classify(scan: &ContainerScan) -> DialectMatch {
    let layout = scan.framing.layout;
    let admission = if layout_is_declared(layout) {
        Admission::Admitted
    } else {
        Admission::AdmittedUnverified {
            nearest: Layout::Unknown.id(),
        }
    };
    let mut declared = BTreeMap::new();
    declared.insert(
        DECLARED_VERSION_LINE.into(),
        scan.framing.version_line.clone(),
    );
    if let Some(legacy) = &scan.framing.legacy_ascii {
        declared.insert(DECLARED_LEGACY_ASCII_SCHEMA.into(), legacy.schema.clone());
        if let Some(release) = &legacy.product_release {
            declared.insert(
                DECLARED_LEGACY_ASCII_PRODUCT_RELEASE.into(),
                release.clone(),
            );
        }
    }
    DialectMatch {
        format: FORMAT.into(),
        dialect: Some(layout.id()),
        declared,
        admission,
    }
}

#[cfg(test)]
mod tests;
