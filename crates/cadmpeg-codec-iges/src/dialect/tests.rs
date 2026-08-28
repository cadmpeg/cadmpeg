// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::loss::IgesLossCode;
use crate::test_support::{fixed_ascii_with_global, point_file_with_global};
use crate::IgesCodec;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use std::io::Cursor;

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    cadmpeg_test_support::assert_registry_closed("iges", &IgesDialect::ALL.map(IgesDialect::id));
}

#[test]
fn the_totality_row_absorbs_the_representation_version_pairs_the_registry_omits() {
    // Fixed ASCII enumerates all eleven flags the version table declares.
    for flag in 1..=11 {
        assert_ne!(
            IgesDialect::from_representation_and_flag(Representation::FixedAscii, flag),
            IgesDialect::Unknown,
            "fixed ASCII flag {flag} must name its own row"
        );
    }
    // Compressed ASCII and Binary enumerate only the witnessed versions.
    for representation in [Representation::CompressedAscii, Representation::Binary] {
        for flag in [6, 8, 9, 10, 11] {
            assert_ne!(
                IgesDialect::from_representation_and_flag(representation, flag),
                IgesDialect::Unknown,
                "{representation:?} flag {flag} must name its own row"
            );
        }
        for flag in [1, 2, 3, 4, 5, 7] {
            assert_eq!(
                IgesDialect::from_representation_and_flag(representation, flag),
                IgesDialect::Unknown,
                "{representation:?} flag {flag} has no declared row"
            );
        }
    }
}

#[test]
fn every_write_target_names_a_fixed_ascii_row() {
    for version in [
        IgesVersion::V4_0,
        IgesVersion::V5_0,
        IgesVersion::V5_1,
        IgesVersion::V5_2,
        IgesVersion::V5_3,
    ] {
        let id = IgesDialect::fixed_ascii(version).id();
        assert_eq!(id.as_str(), format!("iges:{}-fixed-ascii", version.name()));
    }
}

/// A 26-field Global record with `version_flag` substituted for field 23.
///
/// Field 23 is the version flag of IGES 5.3 Table 1. An empty string omits the
/// field, which is the specification's own default case.
fn global_record(version_flag: &str) -> Vec<u8> {
    format!(
        "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,\
         2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,{version_flag},0,0H,0H;"
    )
    .into_bytes()
}

/// Resolves the Global section of a Fixed ASCII file carrying `version_flag`.
///
/// The physical representation does not reach the Global resolver: Compressed
/// ASCII and Binary are normalized to fixed cards before this point, so one
/// resolved Global serves every representation in the matrix below.
fn resolved_global(version_flag: &str) -> crate::global::ResolvedGlobal {
    let bytes = fixed_ascii_with_global(&global_record(version_flag));
    let scan = crate::card::scan(&bytes).unwrap();
    crate::global::parse(&scan).unwrap().0
}

/// Whether `global` charges [`IgesLossCode::SourceDialectUnverified`].
fn global_charges_dialect_unverified(global: &crate::global::ResolvedGlobal) -> bool {
    let expected = IgesLossCode::SourceDialectUnverified
        .note(String::new())
        .code;
    let matched = IgesDialect::classify(Representation::FixedAscii, global);
    dialect_loss(global, &matched).is_some_and(|note| note.code == expected)
}

/// One matrix row: a field-23 declaration and what each representation must
/// classify it as.
struct Case {
    /// Field 23 as written on the card; empty means the field is omitted.
    declaration: &'static str,
    /// The `version_flag` key the match must carry, after the specification
    /// default stands in for an absent or unreadable field.
    declared_flag: &'static str,
    /// Whether field 23 failed to read as an integer.
    unreadable: bool,
    /// Registry id for a Fixed ASCII document with this declaration.
    fixed_id: &'static str,
    /// Registry id for a Compressed ASCII document with this declaration.
    compressed_id: &'static str,
    /// Whether the declared version's Global table is one this codec verified.
    admitted: bool,
}

/// Declarations spanning every arm of the version table.
///
/// Ids come from `docs/dialects.toml`, admission from `VERIFIED_VERSIONS`
/// (4.0, 5.0, 5.1, 5.2, 5.3), and the default of 3 for an absent or unreadable
/// field 23 from IGES 5.3 section 2.2.4.3.23. Flags 12 and 99 sit outside the
/// version table, so no row's `version_flag` discriminant matches them, and the
/// clamp that recovers them is itself a grammar no row declares for them.
const CASES: &[Case] = &[
    Case {
        declaration: "1",
        declared_flag: "1",
        unreadable: false,
        fixed_id: "iges:1.0-fixed-ascii",
        compressed_id: "iges:unknown",
        admitted: false,
    },
    Case {
        declaration: "3",
        declared_flag: "3",
        unreadable: false,
        fixed_id: "iges:2.0-fixed-ascii",
        compressed_id: "iges:unknown",
        admitted: false,
    },
    Case {
        declaration: "6",
        declared_flag: "6",
        unreadable: false,
        fixed_id: "iges:4.0-fixed-ascii",
        compressed_id: "iges:4.0-compressed-ascii",
        admitted: true,
    },
    Case {
        declaration: "11",
        declared_flag: "11",
        unreadable: false,
        fixed_id: "iges:5.3-fixed-ascii",
        compressed_id: "iges:5.3-compressed-ascii",
        admitted: true,
    },
    Case {
        declaration: "12",
        declared_flag: "12",
        unreadable: false,
        fixed_id: "iges:unknown",
        compressed_id: "iges:unknown",
        admitted: false,
    },
    Case {
        declaration: "99",
        declared_flag: "99",
        unreadable: false,
        fixed_id: "iges:unknown",
        compressed_id: "iges:unknown",
        admitted: false,
    },
    Case {
        declaration: "1Hx",
        declared_flag: "3",
        unreadable: true,
        fixed_id: "iges:2.0-fixed-ascii",
        compressed_id: "iges:unknown",
        admitted: false,
    },
    Case {
        declaration: "",
        declared_flag: "3",
        unreadable: false,
        fixed_id: "iges:2.0-fixed-ascii",
        compressed_id: "iges:unknown",
        admitted: false,
    },
];

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    for case in CASES {
        let global = resolved_global(case.declaration);
        let charged = global_charges_dialect_unverified(&global);
        assert_eq!(
            case.admitted, !charged,
            "field 23 {:?}: the case table and the charged loss disagree",
            case.declaration
        );

        for representation in [
            Representation::FixedAscii,
            Representation::CompressedAscii,
            Representation::Binary,
        ] {
            let matched = IgesDialect::classify(representation, &global);
            assert_eq!(
                matched.admission == Admission::Admitted,
                !charged,
                "field 23 {:?} as {representation:?}: admission and the dialect-unverified loss must agree",
                case.declaration
            );
        }
    }
}

#[test]
fn each_declaration_classifies_into_the_row_its_discriminants_match() {
    for case in CASES {
        let global = resolved_global(case.declaration);
        for (representation, expected_id, nearest_id) in [
            (
                Representation::FixedAscii,
                case.fixed_id,
                "iges:5.3-fixed-ascii",
            ),
            (
                Representation::CompressedAscii,
                case.compressed_id,
                "iges:5.3-compressed-ascii",
            ),
        ] {
            let matched = IgesDialect::classify(representation, &global);
            let context = format!("field 23 {:?} as {representation:?}", case.declaration);

            assert_eq!(
                matched.dialect.as_ref().map(DialectId::as_str),
                Some(expected_id),
                "{context}"
            );
            assert_eq!(
                matched.declared[DECLARED_VERSION_FLAG], case.declared_flag,
                "{context}: the declaration is recorded as the source made it"
            );
            assert_eq!(
                matched
                    .declared
                    .contains_key(DECLARED_VERSION_FLAG_DECLARATION),
                case.unreadable,
                "{context}: an unreadable field 23 is described, a readable one is not"
            );
            assert_eq!(
                matched.declared[DECLARED_REPRESENTATION],
                representation.as_str(),
                "{context}"
            );

            let expected_admission = if case.admitted {
                Admission::Admitted
            } else {
                Admission::AdmittedUnverified {
                    nearest: DialectId::pinned(nearest_id),
                }
            };
            assert_eq!(matched.admission, expected_admission, "{context}");
        }
    }
}

/// The one dialect match of a report. The primary-layer invariant makes it the
/// primary layer, so no consumer here indexes by position for any other reason.
fn only_match(dialects: &[DialectMatch]) -> &DialectMatch {
    assert_eq!(dialects.len(), 1, "{dialects:#?}");
    assert_eq!(dialects[0].format, "iges");
    &dialects[0]
}

/// A 26-field Global record carrying `version_flag` in field 23.
fn global_with_version_flag(version_flag: &str) -> Vec<u8> {
    format!(
        "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,\
         2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,{version_flag},0,0H,0H;"
    )
    .into_bytes()
}

fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    assert_eq!(IgesCodec.detect(&bytes), Confidence::High);
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthesized IGES stream should decode")
}

/// Whether `result` charges the dialect-unverified loss.
fn charges_dialect_unverified(result: &cadmpeg_ir::codec::DecodeResult) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::SourceDialectUnverified.kind())
}

#[test]
fn a_legacy_fixed_ascii_declaration_decodes_into_its_own_row_unverified() {
    // ANSI Y14.26M-1981 is version flag 2. It has a registry row of its own and
    // no Global table this codec verified, so identity and admission part
    // company: the row is named, and the admission says the grammar that read
    // the file was a substitute.
    let bytes = point_file_with_global(&global_with_version_flag("2"));
    let decoded = decode(bytes.clone());

    let matched = only_match(&decoded.report().dialects);
    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("iges:ansi-y14.26m-1981-fixed-ascii")
    );
    assert_eq!(
        matched.admission,
        Admission::AdmittedUnverified {
            nearest: DialectId::pinned("iges:5.3-fixed-ascii"),
        }
    );
    assert_eq!(matched.declared["version_flag"], "2");
    assert_eq!(matched.declared["effective_version"], "ANSI-Y14.26M-1981");
    assert!(charges_dialect_unverified(&decoded));

    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(source.dialect, matched.dialect);
    assert_eq!(source.declared, matched.declared);

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(only_match(&summary.dialects), matched);
}

#[test]
fn a_version_flag_outside_the_table_decodes_into_the_totality_row() {
    // Flag 99 clamps to effective version 5.3, but no row declares
    // `version_flag = "99"`, so the document satisfies none of them. The
    // declaration survives in `declared` while the id states only that nothing
    // matched.
    let bytes = point_file_with_global(&global_with_version_flag("99"));
    let decoded = decode(bytes.clone());

    let matched = only_match(&decoded.report().dialects);
    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("iges:unknown")
    );
    assert_eq!(
        matched.admission,
        Admission::AdmittedUnverified {
            nearest: DialectId::pinned("iges:5.3-fixed-ascii"),
        }
    );
    assert_eq!(matched.declared["version_flag"], "99");
    assert_eq!(matched.declared["effective_version"], "5.3");
    assert!(charges_dialect_unverified(&decoded));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(only_match(&summary.dialects), matched);
}

#[test]
fn a_verified_fixed_ascii_declaration_is_admitted_with_no_dialect_loss() {
    // The other side of the biconditional through the whole codec: flag 6 names
    // IGES 4.0, whose Global table this codec verified, so the row is named,
    // the admission is plain, and no dialect loss is charged.
    let bytes = point_file_with_global(&global_with_version_flag("6"));
    let decoded = decode(bytes);

    let matched = only_match(&decoded.report().dialects);
    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("iges:4.0-fixed-ascii")
    );
    assert_eq!(matched.admission, Admission::Admitted);
    assert!(!charges_dialect_unverified(&decoded));
}

#[test]
fn the_totality_row_never_carries_a_verified_admission() {
    // `iges:unknown` states that no row's discriminants matched. A document
    // there was necessarily read with a grammar no row declares for it, so the
    // pair (unknown, Admitted) must be unreachable.
    for case in CASES {
        let global = resolved_global(case.declaration);
        for representation in [
            Representation::FixedAscii,
            Representation::CompressedAscii,
            Representation::Binary,
        ] {
            let matched = IgesDialect::classify(representation, &global);
            if matched.dialect.as_ref().map(DialectId::as_str)
                == Some(IgesDialect::Unknown.id().as_str())
            {
                assert_ne!(
                    matched.admission,
                    Admission::Admitted,
                    "field 23 {:?} as {representation:?}",
                    case.declaration
                );
            }
        }
    }
}
