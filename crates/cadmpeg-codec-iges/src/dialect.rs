// SPDX-License-Identifier: Apache-2.0
//! IGES dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The dialect-id function is internal, `DialectId::pinned` strings are the
//! boundary, [`classify`] is the one construction path, and the vocabulary is
//! closed. Tests close it directly against
//! `docs/dialects.toml`.
//!
//! Identity rows and parser grammars are independent, and IGES shows the gap
//! plainly. The registry enumerates eleven Fixed ASCII versions
//! because IGES 5.3 section 2.2.4.3.23 enumerates eleven version flags; this
//! codec has verified Global tables for five of them
//! ([`crate::global::GlobalTable`] groups the rest as `Legacy`). A document at an
//! unverified version still classifies into its own identity row and is
//! admitted as [`Admission::AdmittedUnverified`], naming the row whose Global
//! table actually parsed it.
//!
//! # The declaration is evidence; the id is identity
//!
//! [`DialectMatch::declared`] records what Global field 23 says.
//! [`DialectMatch::dialect`] records which registry row the document satisfies.
//! They are different statements and a consumer must not join them. A
//! declaration of `99` yields `iges:unknown`, not `iges:5.3-fixed-ascii`,
//! because a row matches only when every one of its discriminants matches and
//! no row declares `version_flag = "99"`. Parse a version out of an id, or
//! expect an id to agree with the `version_flag` beside it, and the answer is
//! wrong for exactly the files whose declarations are wrong.

use crate::global::ResolvedGlobal;
use crate::representation::Representation;
use crate::version::{DialectRecovery, UnverifiedDialectRecovery, VersionFlag};
use crate::IgesVersion;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "iges";

/// The dialect-unverified loss required by a classified Global declaration.
pub(crate) fn dialect_loss(matched: &DialectMatch, global: &ResolvedGlobal) -> Option<LossNote> {
    match matched.admission() {
        Admission::Admitted | Admission::Refused => return None,
        Admission::AdmittedUnverified { .. } => {}
    }
    let declared = global.declared_version_flag();
    let version = global.version_name();
    let (declaration, clamp) = match global.dialect_recovery() {
        DialectRecovery::Unverified(UnverifiedDialectRecovery::UnreadableDeclaration(
            declaration,
        )) => (
            format!(
                "IGES Global field 23 (version flag) is malformed: the declaration {declaration} does not read as an integer, so the specification default {declared}",
            ),
            String::new(),
        ),
        DialectRecovery::Unverified(UnverifiedDialectRecovery::Clamped) => (
            format!("IGES Global version flag {declared}"),
            format!(
                " after the clamp to {} that IGES 5.3 section 2.2.4.3.23 requires of a postprocessor",
                global.effective_version_flag()
            ),
        ),
        DialectRecovery::Unverified(UnverifiedDialectRecovery::UnverifiedVersion) => (
            format!("IGES Global version flag {declared}"),
            String::new(),
        ),
        DialectRecovery::Verified => (
            format!("IGES Global version flag {declared} was admitted as unverified"),
            String::new(),
        ),
    };
    Some(crate::loss::IgesLossCode::SourceDialectUnverified.note(format!(
        "{declaration} names effective specification version {version}{clamp}; this decode interpreted the file with the semantics verified for versions {}",
        IgesVersion::ALL
            .map(IgesVersion::name)
            .join(", ")
    )))
}

/// Key of the physical representation in [`DialectMatch::declared`].
const DECLARED_REPRESENTATION: &str = "representation";
/// Key of Global field 23 as declared, in [`DialectMatch::declared`].
///
/// The specification default 3 stands in for an absent or unreadable field, so
/// this key is the resolved declaration rather than raw bytes. A field 23 that
/// does not read as an integer is described under
/// [`DECLARED_VERSION_FLAG_DECLARATION`].
const DECLARED_VERSION_FLAG: &str = "version_flag";
/// Key of the version after the postprocessor clamp, in
/// [`DialectMatch::declared`].
///
/// This is the sole report location for the effective version.
const DECLARED_EFFECTIVE_VERSION: &str = "effective_version";
/// Key of the numeric version flag after a postprocessor clamp, in
/// [`DialectMatch::declared`]. Absent when no clamp occurred.
const DECLARED_EFFECTIVE_VERSION_FLAG: &str = "effective_version_flag";
/// Key describing a Global field 23 that does not read as an integer, in
/// [`DialectMatch::declared`]. Absent when field 23 reads.
///
/// Not the raw bytes of the field. It is the value the Global resolver
/// recovered: a Hollerith declaration of `1Hx` appears here as its decoded
/// content `x`, and a string carrying a byte the dialect forbids appears as a
/// sentence naming that condition rather than as a declaration at all. Treat it
/// as a description of the defect, not as a transcript of the card.
const DECLARED_VERSION_FLAG_DECLARATION: &str = "version_flag_declaration";

/// The registry row for one physical representation and declared version flag.
///
/// Fixed ASCII has a row for every flag in [`VersionFlag::ALL`]. Compressed
/// ASCII and Binary have rows for the five versions whose Global tables are
/// verified. Every other pair lands on the mandatory totality row.
pub(crate) const fn dialect_id(
    representation: Representation,
    version: Option<VersionFlag>,
) -> DialectId {
    let id = match (representation, version) {
        (Representation::FixedAscii, Some(VersionFlag::V1_0)) => "iges:1.0-fixed-ascii",
        (Representation::FixedAscii, Some(VersionFlag::AnsiY1426M1981)) => {
            "iges:ansi-y14.26m-1981-fixed-ascii"
        }
        (Representation::FixedAscii, Some(VersionFlag::V2_0)) => "iges:2.0-fixed-ascii",
        (Representation::FixedAscii, Some(VersionFlag::V3_0)) => "iges:3.0-fixed-ascii",
        (Representation::FixedAscii, Some(VersionFlag::AsmeAnsiY1426M1987)) => {
            "iges:asme-ansi-y14.26m-1987-fixed-ascii"
        }
        (Representation::FixedAscii, Some(VersionFlag::V4_0)) => "iges:4.0-fixed-ascii",
        (Representation::FixedAscii, Some(VersionFlag::AsmeY1426M1989)) => {
            "iges:asme-y14.26m-1989-fixed-ascii"
        }
        (Representation::FixedAscii, Some(VersionFlag::V5_0)) => "iges:5.0-fixed-ascii",
        (Representation::FixedAscii, Some(VersionFlag::V5_1)) => "iges:5.1-fixed-ascii",
        (Representation::FixedAscii, Some(VersionFlag::V5_2)) => "iges:5.2-fixed-ascii",
        (Representation::FixedAscii, Some(VersionFlag::V5_3)) => "iges:5.3-fixed-ascii",
        (Representation::CompressedAscii, Some(VersionFlag::V4_0)) => "iges:4.0-compressed-ascii",
        (Representation::CompressedAscii, Some(VersionFlag::V5_0)) => "iges:5.0-compressed-ascii",
        (Representation::CompressedAscii, Some(VersionFlag::V5_1)) => "iges:5.1-compressed-ascii",
        (Representation::CompressedAscii, Some(VersionFlag::V5_2)) => "iges:5.2-compressed-ascii",
        (Representation::CompressedAscii, Some(VersionFlag::V5_3)) => "iges:5.3-compressed-ascii",
        (Representation::Binary, Some(VersionFlag::V4_0)) => "iges:4.0-binary",
        (Representation::Binary, Some(VersionFlag::V5_0)) => "iges:5.0-binary",
        (Representation::Binary, Some(VersionFlag::V5_1)) => "iges:5.1-binary",
        (Representation::Binary, Some(VersionFlag::V5_2)) => "iges:5.2-binary",
        (Representation::Binary, Some(VersionFlag::V5_3)) => "iges:5.3-binary",
        _ => "iges:unknown",
    };
    DialectId::pinned(id)
}

/// The Fixed ASCII row for one public write version.
pub(crate) const fn fixed_ascii_id(version: IgesVersion) -> DialectId {
    dialect_id(
        Representation::FixedAscii,
        Some(VersionFlag::from_write_version(version)),
    )
}

/// The row whose Global table parses a document this codec has not verified at
/// its own version.
const fn nearest_verified_id(representation: Representation) -> DialectId {
    match representation {
        Representation::Unknown => dialect_id(Representation::FixedAscii, Some(VersionFlag::V5_3)),
        representation => dialect_id(representation, Some(VersionFlag::V5_3)),
    }
}

/// Classifies one document from its representation and resolved Global facts.
pub(crate) fn classify(representation: Representation, global: &ResolvedGlobal) -> DialectMatch {
    let dialect = dialect_id(representation, global.declared_version());
    let recovery = global.dialect_recovery();
    let mut declared = BTreeMap::new();
    declared.insert(
        DECLARED_REPRESENTATION.into(),
        representation.as_str().into(),
    );
    declared.insert(
        DECLARED_VERSION_FLAG.into(),
        global.declared_version_flag().to_string(),
    );
    declared.insert(
        DECLARED_EFFECTIVE_VERSION.into(),
        global.version_name().to_owned(),
    );
    if matches!(
        recovery,
        DialectRecovery::Unverified(UnverifiedDialectRecovery::Clamped)
    ) {
        declared.insert(
            DECLARED_EFFECTIVE_VERSION_FLAG.into(),
            global.effective_version_flag().to_string(),
        );
    }
    if let Some(text) = global.unreadable_version_declaration() {
        declared.insert(DECLARED_VERSION_FLAG_DECLARATION.into(), text.to_owned());
    }
    if matches!(recovery, DialectRecovery::Verified) {
        DialectMatch::admitted(dialect)
    } else {
        DialectMatch::unverified(dialect, nearest_verified_id(representation))
            .expect("IGES dialect and grammar ids share one format namespace")
    }
    .with_declared(declared)
}

#[cfg(test)]
mod tests;
