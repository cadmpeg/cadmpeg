// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;

/// A document element carrying `schema_version`, with the other declarations
/// fixed.
fn document(schema_version: &str) -> DocumentFacts {
    DocumentFacts {
        id: "document-0".into(),
        schema_version: schema_version.into(),
        file_version: "1".into(),
        program_version: Some("1.1R20260414 (Git shallow)".into()),
        root_name: "Document".into(),
        object_count: 0,
        document_kind: "part".into(),
        domains: Vec::new(),
    }
}

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    cadmpeg_test_support::assert_registry_closed("fcstd", &FcstdDialect::ALL.map(FcstdDialect::id));
}

/// One matrix row: a `SchemaVersion` declaration and what it must classify as.
struct Case {
    /// `Document/@SchemaVersion` as written in the file.
    declaration: &'static str,
    /// Registry id the discriminant matches.
    id: &'static str,
    /// Whether this codec declares a parse strategy for that row.
    admitted: bool,
}

/// Declarations spanning every arm of the schema dispatch.
///
/// Ids come from `docs/dialects.toml`, admission from
/// `persistence::parse_with_context`'s `== "2"` branch and `else` branch.
/// `"04"` and `"10"` parse as unsigned integers — `container::parse_document`
/// requires that much — and still match no row's `schema_version` discriminant.
const CASES: &[Case] = &[
    Case {
        declaration: "2",
        id: "fcstd:schema-2",
        admitted: true,
    },
    Case {
        declaration: "3",
        id: "fcstd:schema-3",
        admitted: true,
    },
    Case {
        declaration: "4",
        id: "fcstd:schema-4",
        admitted: true,
    },
    Case {
        declaration: "0",
        id: "fcstd:unknown",
        admitted: false,
    },
    Case {
        declaration: "04",
        id: "fcstd:unknown",
        admitted: false,
    },
    Case {
        declaration: "5",
        id: "fcstd:unknown",
        admitted: false,
    },
    Case {
        declaration: "10",
        id: "fcstd:unknown",
        admitted: false,
    },
];

#[test]
fn each_declaration_classifies_into_the_row_its_discriminant_matches() {
    for case in CASES {
        let matched = FcstdDialect::classify(
            &document(case.declaration),
            FcstdDialect::from_schema_version(case.declaration),
        );
        let context = format!("SchemaVersion {:?}", case.declaration);

        assert_eq!(matched.format(), FORMAT, "{context}");
        assert_eq!(matched.dialect().as_str(), case.id, "{context}");
        let expected_admission = if case.admitted {
            Admission::Admitted
        } else {
            Admission::AdmittedUnverified {
                using: DialectId::pinned("fcstd:schema-4"),
            }
        };
        assert_eq!(matched.admission(), expected_admission, "{context}");
    }
}

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    let expected = FreecadLossCode::SourceDialectUnverified
        .note(String::new())
        .code;
    for case in CASES {
        let facts = document(case.declaration);
        let matched =
            FcstdDialect::classify(&facts, FcstdDialect::from_schema_version(case.declaration));
        let charged =
            FcstdDialect::dialect_loss(&matched).is_some_and(|note| note.code == expected);
        assert_eq!(
            case.admitted, !charged,
            "SchemaVersion {:?}: the case table and the charged loss disagree",
            case.declaration
        );
        assert_eq!(
            matched.admission() == Admission::Admitted,
            !charged,
            "SchemaVersion {:?}: admission and the dialect-unverified loss must agree",
            case.declaration
        );
    }
}

#[test]
fn the_totality_row_never_carries_a_verified_admission() {
    // `fcstd:unknown` states that no row's discriminant matched. A document
    // there was necessarily read with a vocabulary no row declares for it, so
    // the pair (unknown, Admitted) must be unreachable.
    for case in CASES {
        let matched = FcstdDialect::classify(
            &document(case.declaration),
            FcstdDialect::from_schema_version(case.declaration),
        );
        if matched.dialect().as_str() == FcstdDialect::Unknown.id().as_str() {
            assert_ne!(
                matched.admission(),
                Admission::Admitted,
                "SchemaVersion {:?}",
                case.declaration
            );
        }
    }
}

#[test]
fn the_declared_keys_are_pinned_and_verbatim() {
    let matched = FcstdDialect::classify(&document("4"), FcstdDialect::Schema4);
    assert_eq!(
        matched.declared().keys().collect::<Vec<_>>(),
        ["file_version", "program_version", "schema_version"]
    );
    assert_eq!(matched.declared()[DECLARED_SCHEMA_VERSION], "4");
    assert_eq!(matched.declared()[DECLARED_FILE_VERSION], "1");
    assert_eq!(
        matched.declared()[DECLARED_PROGRAM_VERSION],
        "1.1R20260414 (Git shallow)"
    );

    // `ProgramVersion` has no substituted default anywhere in the codec, so an
    // absent attribute leaves the key out rather than inventing a value.
    let mut facts = document("4");
    facts.program_version = None;
    let matched = FcstdDialect::classify(&facts, FcstdDialect::Schema4);
    assert_eq!(
        matched.declared().keys().collect::<Vec<_>>(),
        ["file_version", "schema_version"]
    );
}
