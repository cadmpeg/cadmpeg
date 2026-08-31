// SPDX-License-Identifier: Apache-2.0
//! CATIA V5 dialect identity: which registry row a `.CATPart` is, and how it
//! was admitted.
//!
//! The `*LossCode` template: the enum is internal to the crate, [`DialectId`]
//! strings are the boundary, [`classify`] is the one construction path, and the
//! vocabulary is closed. The enum is [`Variant`] itself, so this module gives
//! that existing enum a pinned-id surface instead of standing up a second enum
//! that would have to be kept in step with it by hand. Tests close that enum
//! directly against `docs/dialects.toml`.
//!
//! # Identity is structural here, and there is no declaration to disagree with
//!
//! CATIA's storage families carry no version number: they are recognized from
//! container shape, a reconstructed B-rep stream, spine markers, table
//! delimiters, and a record-family census — all read by
//! [`crate::container::scan_bytes`] before any parse strategy is chosen (B1).
//! So unlike IGES, identity is not a declaration that can be wrong; the file
//! either exhibits a family's invariants or it does not.
//!
//! The one declaration the codec reads, the `LastSaveVersion` release tuple, is
//! provenance: it is not an argument to `identify_variant` and appears in no
//! conditional in the crate. It is recorded in
//! [`DialectMatch::declared`] as evidence and branched on nowhere.
//!
//! [`Variant::Unknown`] uses the metadata-IR fallback. Its
//! [`Admission::AdmittedUnverified`] value names no substituted grammar; no
//! recognized family grammar is applied.

use crate::container::ContainerScan;
use crate::loss::CatiaLossCode;
use crate::variant::Variant;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "catia";

/// Key of the `LastSaveVersion` generation number in [`DialectMatch::declared`].
const DECLARED_VERSION: &str = "last_save_version";
/// Key of the `LastSaveVersion` release number in [`DialectMatch::declared`].
const DECLARED_RELEASE: &str = "last_save_release";
/// Key of the `LastSaveVersion` service-pack number in [`DialectMatch::declared`].
const DECLARED_SERVICE_PACK: &str = "last_save_service_pack";
/// Key of the `LastSaveVersion` hot-fix number in [`DialectMatch::declared`].
const DECLARED_HOT_FIX: &str = "last_save_hot_fix";
/// Key of the `LastSaveVersion` build-date string in [`DialectMatch::declared`].
const DECLARED_BUILD_DATE: &str = "last_save_build_date";

impl Variant {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 7] = [
        Self::StandardNested,
        Self::FbbOnly,
        Self::ZeroEntity,
        Self::FloatPackedInnerNoFbb,
        Self::E5Stream,
        Self::InnerNoDirectory,
        Self::Unknown,
    ];

    /// The pinned registry id. The only string boundary this enum has.
    ///
    /// This is the sole serialized spelling of the storage family. Human
    /// descriptions remain prose; source metadata and annotations do not carry
    /// a second bare variant token.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(match self {
            Self::StandardNested => "catia:standard-nested",
            Self::FbbOnly => "catia:fbb-only",
            Self::ZeroEntity => "catia:zero-entity",
            Self::FloatPackedInnerNoFbb => "catia:float-packed-inner-no-fbb",
            Self::E5Stream => "catia:e5-stream",
            Self::InnerNoDirectory => "catia:inner-no-directory",
            Self::Unknown => "catia:unknown",
        })
    }
}

/// How this codec admitted a document identified as `variant`.
///
/// The one admission predicate in this crate. Both [`classify`] and
/// [`dialect_loss`] read it, so a report's `admission` and its charged loss
/// cannot disagree.
///
/// Each of the six decoding families has at least one applicable route in
/// [`crate::families::ROUTES`], and that route is the strategy the registry
/// declares for its row: [`Admission::Admitted`]. Whether the route then yields
/// a transferable model is content-conditioned, which B2 puts inside the
/// dialect — a route returning `None` is a loss within an admitted dialect, and
/// the existing geometry and topology losses already say so.
///
/// [`Variant::Unknown`] matches no route at all, so no declared strategy was
/// applied to it: [`Admission::AdmittedUnverified`].
pub(crate) fn admission(variant: Variant) -> Admission {
    match variant {
        Variant::StandardNested
        | Variant::FbbOnly
        | Variant::ZeroEntity
        | Variant::FloatPackedInnerNoFbb
        | Variant::E5Stream
        | Variant::InnerNoDirectory => Admission::Admitted,
        Variant::Unknown => Admission::AdmittedUnverified { using: None },
    }
}

/// The dialect-unverified loss (§7), charged exactly on
/// [`Admission::AdmittedUnverified`].
///
/// `None` exactly when the classified match is [`Admission::Admitted`]. The
/// loss reads the admission already built by [`classify`] and does not
/// recompute it from the variant.
///
/// This is a *dialect* loss and is disjoint from
/// [`CatiaLossCode::GeometryBrepNotTransferred`] and
/// [`CatiaLossCode::TopologyGraphNotBuilt`], which state what was not
/// transferred out of an identified layout. This one states that the layout was
/// never identified.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    let Admission::AdmittedUnverified { .. } = matched.admission() else {
        return None;
    };
    Some(CatiaLossCode::SourceDialectUnverified.note(format!(
        "This container matched no CATIA V5 storage family's structural invariants, so it \
is `{}`. No decode route declares a grammar for that row, and no declared \
                 dialect grammar was substituted; the file was \
                 admitted under the metadata-IR fallback, which enumerates the container and \
                 retains the source bytes without applying any family's record grammar.",
        matched.dialect()
    )))
}

/// The `LastSaveVersion` tuple the summary-information record declared.
///
/// Recorded verbatim in the sense the source allows: `<Version>`, `<Release>`,
/// `<ServicePack>`, and `<HotFix>` are decimal ASCII that
/// `container::parse_last_save_version` resolves to integers — the whole tuple
/// is absent unless all four read — and `<BuildDate>` is carried through as the
/// string it is. Nothing here is branched on (§3.4): it is provenance recorded
/// as evidence.
fn declared(scan: &ContainerScan) -> BTreeMap<String, String> {
    let mut declared = BTreeMap::new();
    if let Some(version) = &scan.last_save_version {
        declared.insert(DECLARED_VERSION.into(), version.version.to_string());
        declared.insert(DECLARED_RELEASE.into(), version.release.to_string());
        declared.insert(
            DECLARED_SERVICE_PACK.into(),
            version.service_pack.to_string(),
        );
        declared.insert(DECLARED_HOT_FIX.into(), version.hot_fix.to_string());
        declared.insert(DECLARED_BUILD_DATE.into(), version.build_date.clone());
    }
    declared
}

/// Classifies one scanned container. The single construction path for a
/// [`DialectMatch`] in this codec, so a classification bug and the report can
/// never disagree.
///
/// Identity is [`ContainerScan::variant`], the structural family the scan
/// resolved; admission is [`admission`]. Neither is computed from the other.
pub(crate) fn classify(scan: &ContainerScan) -> DialectMatch {
    DialectMatch::from_admission(scan.variant.id(), admission(scan.variant))
        .expect("CATIA dialect admissions use only CATIA grammar ids")
        .with_declared(declared(scan))
}

#[cfg(test)]
mod tests;
