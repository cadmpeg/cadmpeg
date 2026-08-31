// SPDX-License-Identifier: Apache-2.0
//! IGES dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, `DialectId::pinned` strings
//! are the boundary, [`IgesDialect::classify`] is the one construction path,
//! and the vocabulary is closed. `docs/dialects.toml` generates the exhaustive
//! row list in `dialect/generated.rs`.
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

use crate::global::{DialectRecovery, ResolvedGlobal, VersionFlag};
use crate::representation::Representation;
use crate::IgesVersion;
use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};
use cadmpeg_ir::codec::TargetDescriptor;
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

#[cfg(test)]
mod generated;

/// The format layer every match here classifies.
pub(crate) const FORMAT: &str = "iges";

/// The synthesis catalog: what the semantic writer can produce for any input.
///
/// The writer emits Fixed ASCII only, so the five verified Fixed ASCII rows are
/// the whole of it. Compressed ASCII and Binary are absent by construction, and
/// so are the seven unverified Fixed ASCII rows: no input makes the writer emit
/// them. Those dialects are still writable, by preserving a retained source
/// image under `TargetRequest::Inherit` — preservation, not synthesis.
///
/// The alias of each row is its bare version, so `--to 5.3` and
/// `--to iges:5.3-fixed-ascii` name the same row.
pub(crate) const TARGETS: &[TargetDescriptor] = &[
    TargetDescriptor {
        id: IgesDialect::V4_0FixedAscii.pinned(),
        label: "IGES 4.0 Fixed ASCII",
        aliases: &["4.0"],
        default: false,
    },
    TargetDescriptor {
        id: IgesDialect::V5_0FixedAscii.pinned(),
        label: "IGES 5.0 Fixed ASCII",
        aliases: &["5.0"],
        default: false,
    },
    TargetDescriptor {
        id: IgesDialect::V5_1FixedAscii.pinned(),
        label: "IGES 5.1 Fixed ASCII",
        aliases: &["5.1"],
        default: false,
    },
    TargetDescriptor {
        id: IgesDialect::V5_2FixedAscii.pinned(),
        label: "IGES 5.2 Fixed ASCII",
        aliases: &["5.2"],
        default: false,
    },
    TargetDescriptor {
        id: IgesDialect::V5_3FixedAscii.pinned(),
        label: "IGES 5.3 Fixed ASCII",
        aliases: &["5.3"],
        default: true,
    },
];

/// The write version represented by a canonical catalog entry.
pub(crate) fn target_version(target: &TargetDescriptor) -> IgesVersion {
    IgesVersion::ALL
        .into_iter()
        .find(|version| IgesDialect::fixed_ascii(*version).pinned() == target.id)
        .expect("IGES TARGETS entries map to IgesVersion::ALL")
}

/// The dialect-unverified loss required by a classified Global declaration.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    let Admission::AdmittedUnverified { .. } = matched.admission() else {
        return None;
    };
    let declared = matched
        .declared()
        .get(DECLARED_VERSION_FLAG)
        .map_or("absent", String::as_str);
    let version = matched
        .declared()
        .get(DECLARED_EFFECTIVE_VERSION)
        .map_or("unknown", String::as_str);
    let declaration = match matched.declared().get(DECLARED_VERSION_FLAG_DECLARATION) {
        Some(text) => format!(
            "IGES Global field 23 (version flag) is malformed: the declaration {text} does not read as an integer, so the specification default {declared}"
        ),
        None => format!("IGES Global version flag {declared}"),
    };
    let clamp = matched
        .declared()
        .get(DECLARED_EFFECTIVE_VERSION_FLAG)
        .map_or_else(String::new, |effective| {
            format!(
                " after the clamp to {effective} that IGES 5.3 section 2.2.4.3.23 requires of a postprocessor"
            )
        });
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

/// One row of `docs/dialects.toml` under the `iges` namespace.
///
/// Fixed ASCII carries a variant per declared version flag the specification
/// enumerates, 1 through 11. Compressed ASCII and Binary carry only the versions
/// the registry cites a specification for. Every other declaration — a flag the
/// version table does not contain, or a representation and version pair no row
/// states — lands on [`IgesDialect::Unknown`], the mandatory totality row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum IgesDialect {
    V1_0FixedAscii,
    AnsiY1426M1981FixedAscii,
    V2_0FixedAscii,
    V3_0FixedAscii,
    AsmeAnsiY1426M1987FixedAscii,
    V4_0FixedAscii,
    AsmeY1426M1989FixedAscii,
    V5_0FixedAscii,
    V5_1FixedAscii,
    V5_2FixedAscii,
    V5_3FixedAscii,
    V4_0CompressedAscii,
    V5_0CompressedAscii,
    V5_1CompressedAscii,
    V5_2CompressedAscii,
    V5_3CompressedAscii,
    V4_0Binary,
    V5_0Binary,
    V5_1Binary,
    V5_2Binary,
    V5_3Binary,
    Unknown,
}

impl IgesDialect {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 22] = [
        Self::V1_0FixedAscii,
        Self::AnsiY1426M1981FixedAscii,
        Self::V2_0FixedAscii,
        Self::V3_0FixedAscii,
        Self::AsmeAnsiY1426M1987FixedAscii,
        Self::V4_0FixedAscii,
        Self::AsmeY1426M1989FixedAscii,
        Self::V5_0FixedAscii,
        Self::V5_1FixedAscii,
        Self::V5_2FixedAscii,
        Self::V5_3FixedAscii,
        Self::V4_0CompressedAscii,
        Self::V5_0CompressedAscii,
        Self::V5_1CompressedAscii,
        Self::V5_2CompressedAscii,
        Self::V5_3CompressedAscii,
        Self::V4_0Binary,
        Self::V5_0Binary,
        Self::V5_1Binary,
        Self::V5_2Binary,
        Self::V5_3Binary,
        Self::Unknown,
    ];

    /// The pinned registry id. The only string boundary this enum has.
    pub(crate) const fn id(self) -> DialectId {
        DialectId::pinned(self.pinned())
    }

    /// The pinned registry id as a static string, for the write-target catalog
    /// and for [`crate::IgesVersion::target`].
    pub(crate) const fn pinned(self) -> &'static str {
        match self {
            Self::V1_0FixedAscii => "iges:1.0-fixed-ascii",
            Self::AnsiY1426M1981FixedAscii => "iges:ansi-y14.26m-1981-fixed-ascii",
            Self::V2_0FixedAscii => "iges:2.0-fixed-ascii",
            Self::V3_0FixedAscii => "iges:3.0-fixed-ascii",
            Self::AsmeAnsiY1426M1987FixedAscii => "iges:asme-ansi-y14.26m-1987-fixed-ascii",
            Self::V4_0FixedAscii => "iges:4.0-fixed-ascii",
            Self::AsmeY1426M1989FixedAscii => "iges:asme-y14.26m-1989-fixed-ascii",
            Self::V5_0FixedAscii => "iges:5.0-fixed-ascii",
            Self::V5_1FixedAscii => "iges:5.1-fixed-ascii",
            Self::V5_2FixedAscii => "iges:5.2-fixed-ascii",
            Self::V5_3FixedAscii => "iges:5.3-fixed-ascii",
            Self::V4_0CompressedAscii => "iges:4.0-compressed-ascii",
            Self::V5_0CompressedAscii => "iges:5.0-compressed-ascii",
            Self::V5_1CompressedAscii => "iges:5.1-compressed-ascii",
            Self::V5_2CompressedAscii => "iges:5.2-compressed-ascii",
            Self::V5_3CompressedAscii => "iges:5.3-compressed-ascii",
            Self::V4_0Binary => "iges:4.0-binary",
            Self::V5_0Binary => "iges:5.0-binary",
            Self::V5_1Binary => "iges:5.1-binary",
            Self::V5_2Binary => "iges:5.2-binary",
            Self::V5_3Binary => "iges:5.3-binary",
            Self::Unknown => "iges:unknown",
        }
    }

    /// The Fixed ASCII row for a write target.
    ///
    /// The semantic writer emits Fixed ASCII only, so this is the whole target
    /// catalog: [`IgesVersion`] and the Fixed ASCII rows are the same five
    /// versions.
    pub(crate) const fn fixed_ascii(version: IgesVersion) -> Self {
        Self::from_representation_and_version(
            Representation::FixedAscii,
            Some(VersionFlag::from_write_version(version)),
        )
    }

    /// The row for a representation at a declared version flag, or
    /// [`Self::Unknown`] where the registry declares no such pair.
    ///
    /// A registry row matches only when *every* one of its discriminants
    /// matches, and each row above carries `version_flag` alongside
    /// `effective_version`. A flag of 12 or 99 has the effective version of
    /// `iges:5.3-fixed-ascii` but not its `version_flag`, so it matches that row
    /// no more than a flag of 0 does: both land on [`Self::Unknown`], whose
    /// residual discriminant is exactly "matches no row above". Keying identity
    /// on the clamped flag instead would file such a document under a row whose
    /// own discriminant contradicts what the file declares.
    const fn from_representation_and_version(
        representation: Representation,
        version: Option<VersionFlag>,
    ) -> Self {
        match (representation, version) {
            (Representation::FixedAscii, Some(VersionFlag::V1_0)) => Self::V1_0FixedAscii,
            (Representation::FixedAscii, Some(VersionFlag::AnsiY1426M1981)) => {
                Self::AnsiY1426M1981FixedAscii
            }
            (Representation::FixedAscii, Some(VersionFlag::V2_0)) => Self::V2_0FixedAscii,
            (Representation::FixedAscii, Some(VersionFlag::V3_0)) => Self::V3_0FixedAscii,
            (Representation::FixedAscii, Some(VersionFlag::AsmeAnsiY1426M1987)) => {
                Self::AsmeAnsiY1426M1987FixedAscii
            }
            (Representation::FixedAscii, Some(VersionFlag::V4_0)) => Self::V4_0FixedAscii,
            (Representation::FixedAscii, Some(VersionFlag::AsmeY1426M1989)) => {
                Self::AsmeY1426M1989FixedAscii
            }
            (Representation::FixedAscii, Some(VersionFlag::V5_0)) => Self::V5_0FixedAscii,
            (Representation::FixedAscii, Some(VersionFlag::V5_1)) => Self::V5_1FixedAscii,
            (Representation::FixedAscii, Some(VersionFlag::V5_2)) => Self::V5_2FixedAscii,
            (Representation::FixedAscii, Some(VersionFlag::V5_3)) => Self::V5_3FixedAscii,
            (Representation::CompressedAscii, Some(VersionFlag::V4_0)) => Self::V4_0CompressedAscii,
            (Representation::CompressedAscii, Some(VersionFlag::V5_0)) => Self::V5_0CompressedAscii,
            (Representation::CompressedAscii, Some(VersionFlag::V5_1)) => Self::V5_1CompressedAscii,
            (Representation::CompressedAscii, Some(VersionFlag::V5_2)) => Self::V5_2CompressedAscii,
            (Representation::CompressedAscii, Some(VersionFlag::V5_3)) => Self::V5_3CompressedAscii,
            (Representation::Binary, Some(VersionFlag::V4_0)) => Self::V4_0Binary,
            (Representation::Binary, Some(VersionFlag::V5_0)) => Self::V5_0Binary,
            (Representation::Binary, Some(VersionFlag::V5_1)) => Self::V5_1Binary,
            (Representation::Binary, Some(VersionFlag::V5_2)) => Self::V5_2Binary,
            (Representation::Binary, Some(VersionFlag::V5_3)) => Self::V5_3Binary,
            _ => Self::Unknown,
        }
    }

    /// The row whose Global table parses a document this codec has not verified
    /// at its own version.
    ///
    /// [`crate::global::GlobalTable::Legacy`] reads the 26-field Global table, which
    /// is the table of 5.1, 5.2, and 5.3. 5.3 is the newest of the three and the
    /// one the codec's other defaults follow, so it names the strategy used.
    const fn nearest_verified(representation: Representation) -> Self {
        match representation {
            Representation::CompressedAscii => Self::V5_3CompressedAscii,
            Representation::Binary => Self::V5_3Binary,
            // `Representation::Unknown` never reaches classification: `detect`
            // reports `Confidence::No` for it and both `inspect_impl` and
            // `decode_impl` refuse it before a Global table is read. It shares
            // the Fixed ASCII answer so this stays a total function.
            Representation::FixedAscii | Representation::Unknown => Self::V5_3FixedAscii,
        }
    }

    /// Classifies one document. The single construction path for a
    /// [`DialectMatch`] in this codec, so a classification bug and the report
    /// can never disagree.
    ///
    /// Identity is the row whose discriminants the document satisfies, and
    /// admission is [`ResolvedGlobal::dialect_recovery`]. The two are computed
    /// from one predicate each and never from each other: a document can carry a
    /// registry row of its own while its bytes are read with a newer grammar,
    /// which is the whole legacy Fixed ASCII range.
    ///
    /// `Admission::Admitted` holds exactly when [`dialect_loss`] is `None`,
    /// because the loss function reads the admission constructed here instead
    /// of recomputing recovery. That biconditional is what the decode policy
    /// requires, and it is structural here rather than maintained by hand.
    pub(crate) fn classify(
        representation: Representation,
        global: &ResolvedGlobal,
    ) -> DialectMatch {
        let dialect =
            Self::from_representation_and_version(representation, global.declared_version());
        let admission = if global.dialect_recovery() == DialectRecovery::Verified {
            Admission::Admitted
        } else {
            Admission::AdmittedUnverified {
                using: Some(Self::nearest_verified(representation).id()),
            }
        };
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
        if global.dialect_recovery() == DialectRecovery::Clamped {
            declared.insert(
                DECLARED_EFFECTIVE_VERSION_FLAG.into(),
                global.effective_version_flag().to_string(),
            );
        }
        if let Some(text) = global.unreadable_version_declaration() {
            declared.insert(DECLARED_VERSION_FLAG_DECLARATION.into(), text.to_owned());
        }
        DialectMatch::layer(dialect.id(), declared, admission)
            .expect("IGES classifier produced an invalid dialect match")
    }
}

#[cfg(test)]
mod tests;
