// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::{
    FcstdDialect, DECLARED_FILE_VERSION, DECLARED_PROGRAM_VERSION, DECLARED_SCHEMA_VERSION, FORMAT,
};
use crate::loss::FreecadLossCode;
use crate::native::DocumentFacts;
use cadmpeg_core::dialect::{Admission, Grammar};

#[test]
fn enum_and_registry_rows_are_closed_bidirectionally() -> Result<(), Box<dyn std::error::Error>> {
    cadmpeg_test_support::assert_dialect_rows_closed(
        &FcstdDialect::ALL.map(FcstdDialect::id),
        FORMAT,
    )?;
    Ok(())
}

/// A document element with the declarations this codec projects, fixed.
fn document() -> DocumentFacts {
    DocumentFacts {
        id: "document-0".into(),
        file_version: "1".to_owned().try_into().unwrap(),
        program_version: Some("1.1R20260414 (Git shallow)".into()),
        root_name: "Document".into(),
        object_count: 0,
        domains: Vec::new(),
    }
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
/// Ids come from `crates/cadmpeg-registry/docs/dialects.toml`, admission from
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
            &cadmpeg_test_support::service_decode_context(),
            &document(),
            case.declaration,
        )
        .unwrap();
        let context = format!("SchemaVersion {:?}", case.declaration);

        assert_eq!(matched.format(), FORMAT, "{context}");
        assert_eq!(matched.dialect().as_str(), case.id, "{context}");
        let expected_admission = if case.admitted {
            Admission::Admitted
        } else {
            Admission::Unverified {
                using: Grammar::of(
                    &cadmpeg_test_support::service_decode_context(),
                    &FcstdDialect::Schema4.id(),
                )
                .unwrap(),
            }
        };
        assert_eq!(*matched.admission(), expected_admission, "{context}");
    }
}

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    let expected = FreecadLossCode::SourceDialectUnverified
        .note(String::new())
        .code;
    for case in CASES {
        let facts = document();
        let matched = FcstdDialect::classify(
            &cadmpeg_test_support::service_decode_context(),
            &facts,
            case.declaration,
        )
        .unwrap();
        let charged =
            FcstdDialect::dialect_loss(&matched).is_some_and(|note| note.code == expected);
        assert_eq!(
            case.admitted, !charged,
            "SchemaVersion {:?}: the case table and the charged loss disagree",
            case.declaration
        );
        assert_eq!(
            *matched.admission() == Admission::Admitted,
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
            &cadmpeg_test_support::service_decode_context(),
            &document(),
            case.declaration,
        )
        .unwrap();
        if matched.dialect().as_str() == FcstdDialect::Unknown.id().as_str() {
            assert_ne!(
                *matched.admission(),
                Admission::Admitted,
                "SchemaVersion {:?}",
                case.declaration
            );
        }
    }
}

#[test]
fn the_declared_keys_are_pinned_and_verbatim() {
    let matched = FcstdDialect::classify(
        &cadmpeg_test_support::service_decode_context(),
        &document(),
        "4",
    )
    .unwrap();
    assert_eq!(
        matched
            .declared()
            .keys()
            .map(cadmpeg_core::text::NonBlankString::as_str)
            .collect::<Vec<_>>(),
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
    let mut facts = document();
    facts.program_version = None;
    let matched =
        FcstdDialect::classify(&cadmpeg_test_support::service_decode_context(), &facts, "4")
            .unwrap();
    assert_eq!(
        matched
            .declared()
            .keys()
            .map(cadmpeg_core::text::NonBlankString::as_str)
            .collect::<Vec<_>>(),
        ["file_version", "schema_version"]
    );
}

#[test]
fn dialect_declarations_refuse_before_copy_and_insertion() {
    let facts = document();
    for operation in [
        "FreeCAD dialect schema declaration",
        "FreeCAD dialect file declaration",
        "FreeCAD dialect program declaration",
    ] {
        crate::test_support::assert_retained_refusal_at(&[], operation, |ctx| {
            FcstdDialect::classify(ctx, &facts, "04")
        });
        crate::test_support::refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            &[],
            operation,
            |ctx| FcstdDialect::classify(ctx, &facts, "04"),
        );
    }
    crate::test_support::assert_collection_refusal_at(
        &[],
        "FreeCAD dialect declaration entries",
        |ctx| FcstdDialect::classify(ctx, &facts, "04"),
    );
}

#[test]
fn dialect_loss_lookup_and_message_refuse_at_work_and_storage_boundaries() {
    let matched = FcstdDialect::classify(
        &cadmpeg_test_support::service_decode_context(),
        &document(),
        "04",
    )
    .unwrap();
    let charged = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        FcstdDialect::dialect_loss_with(
            &matched,
            |declared, key| ctx.get_btree_map(declared, key, "FreeCAD dialect loss declaration"),
            |message| ctx.format_retained(message, "FreeCAD dialect loss message"),
        )
    };
    crate::test_support::with_service_context(&[], |ctx| {
        assert_eq!(charged(ctx).unwrap(), FcstdDialect::dialect_loss(&matched));
    });
    for operation in [
        "FreeCAD dialect loss declaration",
        "FreeCAD dialect loss message",
    ] {
        crate::test_support::refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            &[],
            operation,
            charged,
        );
    }
    crate::test_support::assert_retained_refusal_at(&[], "FreeCAD dialect loss message", charged);
}
