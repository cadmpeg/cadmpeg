// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.
//!
//! No `.ipt` or `.iam` file exists in this repository, so every path below is
//! driven from a synthetic document built by [`crate::test_support`]. The
//! declarations are the only thing that varies between them.

#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::dialect::Admission;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::LossNote;

use super::*;
use crate::test_support::{
    fixture, primary_envelope_fixture_with, primary_envelope_fixture_with_broken_database,
    primary_envelope_fixture_with_broken_metadata,
    primary_envelope_fixture_with_unavailable_carrier, EnvelopeDeclarations,
};
use crate::InventorCodec;

#[test]
fn enum_and_registry_rows_are_closed_bidirectionally() {
    cadmpeg_test_support::assert_dialect_rows_closed(
        &InventorDialect::ALL.map(InventorDialect::id),
        FORMAT,
    );
}

/// One matrix row: a document's declarations and what they must classify as.
struct Case {
    /// What the test is about, for assertion context.
    label: &'static str,
    /// `None` builds the structural fixture, which declares nothing at all.
    declarations: Option<EnvelopeDeclarations>,
    /// Registry id the document must classify into.
    id: &'static str,
    /// Whether every declaration selects a grammar this codec implements.
    admitted: bool,
    /// The `rse_db_schema` entry, absent when the document declares no schema.
    schema: Option<&'static str>,
    /// The `meta_stream_version` entry, absent when no metadata stream declares one.
    meta_version: Option<&'static str>,
}

/// Declarations spanning both gates, in both directions.
///
/// Both gates degrade rather than refuse, so every row here decodes; only the
/// classification and the loss differ.
const CASES: &[Case] = &[
    Case {
        label: "no RSeDb and no segment: nothing is declared",
        declarations: None,
        id: "inventor:unknown",
        admitted: false,
        schema: None,
        meta_version: None,
    },
    Case {
        label: "schema 31 and Meta Stream version 8",
        declarations: Some(EnvelopeDeclarations {
            schema: 31,
            meta_marker: "RSe Meta Stream Version 8",
            meta_version: 8,
        }),
        id: "inventor:cfb3-rse31-meta8",
        admitted: true,
        schema: Some("31"),
        meta_version: Some("8"),
    },
    Case {
        label: "an unimplemented RSeDb schema",
        declarations: Some(EnvelopeDeclarations {
            schema: 12,
            meta_marker: "RSe Meta Stream Version 8",
            meta_version: 8,
        }),
        id: "inventor:unknown",
        admitted: false,
        schema: Some("12"),
        meta_version: Some("8"),
    },
    Case {
        label: "the verified marker with an unimplemented version word",
        declarations: Some(EnvelopeDeclarations {
            schema: 31,
            meta_marker: "RSe Meta Stream Version 8",
            meta_version: 9,
        }),
        id: "inventor:unknown",
        admitted: false,
        schema: Some("31"),
        meta_version: Some("9"),
    },
    Case {
        label: "an unimplemented marker with the verified version word",
        declarations: Some(EnvelopeDeclarations {
            schema: 31,
            meta_marker: "RSe Meta Stream Version 9",
            meta_version: 8,
        }),
        id: "inventor:unknown",
        admitted: false,
        schema: Some("31"),
        meta_version: Some("8"),
    },
];

impl Case {
    fn bytes(&self) -> Vec<u8> {
        match self.declarations {
            Some(declarations) => primary_envelope_fixture_with(declarations),
            None => fixture(true),
        }
    }
}

/// The one match a decode of `bytes` reports, with the losses beside it.
fn decoded(bytes: &[u8]) -> (DialectMatch, Vec<LossNote>) {
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("both version gates degrade rather than refuse");
    let report = decoded.report();
    // The report also carries the non-primary `acis:` kernel layer. These
    // fixtures carry an ASM carrier, which is admitted at every save format, so
    // the only dialect-unverified charge they can raise is the host's.
    let primary = report
        .dialects()
        .as_ref()
        .unwrap_or_else(|| panic!("one primary layer, got {:#?}", report.dialects()))
        .primary();
    (primary.clone(), report.losses.clone())
}

/// Whether `losses` charges [`InventorLossCode::SourceDialectUnverified`].
fn charges_dialect_unverified(losses: &[LossNote]) -> bool {
    let expected = InventorLossCode::SourceDialectUnverified.kind();
    losses.iter().any(|loss| loss.code == expected)
}

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    for case in CASES {
        let (matched, losses) = decoded(&case.bytes());
        let charged = charges_dialect_unverified(&losses);
        assert_eq!(
            case.admitted, !charged,
            "{}: the case table and the charged loss disagree",
            case.label
        );
        assert_eq!(
            matched.admission() == &Admission::Admitted,
            !charged,
            "{}: admission and the dialect-unverified loss must agree",
            case.label
        );
    }
}

#[test]
fn a_broken_schema_31_stream_keeps_its_declaration_in_the_dialect_reason() {
    let (matched, losses) = decoded(&primary_envelope_fixture_with_broken_database());
    assert_eq!(matched.declared()[DECLARED_RSE_DB_SCHEMA], "31");
    assert!(matches!(matched.admission(), Admission::Unverified { .. }));
    assert_eq!(matched.dialect().as_str(), "inventor:cfb3-rse31-meta8");
    let loss = losses
        .iter()
        .find(|loss| loss.code == InventorLossCode::SourceDialectUnverified.kind())
        .expect("unframed schema-31 recovery loss");
    assert!(loss.message.contains("RSe database schema 31 is declared"));
    assert!(!loss
        .message
        .contains("no RSe database stream declares a schema"));
}

#[test]
fn a_broken_verified_meta_stream_keeps_its_declaration_but_is_not_admitted() {
    let (matched, losses) = decoded(&primary_envelope_fixture_with_broken_metadata());
    assert_eq!(
        matched.declared()[DECLARED_META_STREAM_MARKER],
        MetaStreamDeclaration::VERIFIED_MARKER
    );
    assert_eq!(matched.declared()[DECLARED_META_STREAM_VERSION], "8");
    assert_eq!(matched.dialect().as_str(), "inventor:cfb3-rse31-meta8");
    assert!(matches!(matched.admission(), Admission::Unverified { .. }));
    let loss = losses
        .iter()
        .find(|loss| loss.code == InventorLossCode::SourceDialectUnverified.kind())
        .expect("unframed version-8 metadata recovery loss");
    assert!(loss.message.contains(
        "RSe segment metadata marker \"RSe Meta Stream Version 8\" version 8 is declared"
    ));
    assert!(!loss
        .message
        .contains("no RSe segment metadata stream declares a marker and version"));
}

#[test]
fn each_document_classifies_into_the_row_its_declarations_match() {
    for case in CASES {
        let (matched, _) = decoded(&case.bytes());
        assert_eq!(matched.dialect().as_str(), case.id, "{}", case.label);

        let expected_admission = if case.admitted {
            Admission::Admitted
        } else {
            Admission::Unverified {
                using: cadmpeg_core::dialect::Grammar::of(&InventorDialect::Cfb3Rse31Meta8.id()),
            }
        };
        assert_eq!(matched.admission(), &expected_admission, "{}", case.label);
        if !case.admitted {
            assert_eq!(
                matched.using(),
                Some(InventorDialect::Cfb3Rse31Meta8.id()),
                "{}",
                case.label
            );
        }

        assert_eq!(
            matched.declared()[DECLARED_CFB_MAJOR_VERSION],
            "3",
            "{}: the CFB major version is recorded, never gated on",
            case.label
        );
        assert_eq!(
            matched
                .declared()
                .get(DECLARED_RSE_DB_SCHEMA)
                .map(String::as_str),
            case.schema,
            "{}: the schema is recorded as the stream declared it",
            case.label
        );
        assert_eq!(
            matched
                .declared()
                .get(DECLARED_META_STREAM_VERSION)
                .map(String::as_str),
            case.meta_version,
            "{}: the metadata version is recorded as the stream declared it",
            case.label
        );
        assert_eq!(
            matched
                .declared()
                .get(DECLARED_META_STREAM_MARKER)
                .map(String::as_str),
            case.declarations
                .map(|declarations| declarations.meta_marker),
            "{}: the metadata marker is recorded verbatim",
            case.label
        );
    }
}

#[test]
fn the_totality_row_never_carries_a_verified_admission() {
    // `inventor:unknown` states that no declared row's discriminants matched. A
    // document there was necessarily read with a grammar no row declares for
    // it, so the pair (unknown, Admitted) must be unreachable.
    for case in CASES {
        let (matched, _) = decoded(&case.bytes());
        if matched.dialect().as_str() == InventorDialect::Unknown.id().as_str() {
            assert_ne!(matched.admission(), &Admission::Admitted, "{}", case.label);
        }
    }
}

#[test]
fn inspect_and_decode_report_the_same_match_and_the_source_mirrors_it() {
    for case in CASES {
        let bytes = case.bytes();
        let (matched, decoded_losses) = decoded(&bytes);

        let summary = InventorCodec
            .inspect(
                &mut std::io::Cursor::new(&bytes),
                &InspectOptions::default(),
            )
            .expect("the synthetic document inspects");
        let layers = summary
            .dialects()
            .expect("Inventor inspection reports dialect layers");
        assert_eq!(layers.primary(), &matched, "{}", case.label);
        let inspected_classification = summary
            .losses
            .iter()
            .filter(|loss| {
                loss.code == InventorLossCode::SourceDialectUnverified.kind()
                    || loss.code == InventorLossCode::KernelDialectUnverified.kind()
                    || loss.code == InventorLossCode::KernelCarrierUnparseable.kind()
            })
            .collect::<Vec<_>>();
        let decoded_classification = decoded_losses
            .iter()
            .filter(|loss| {
                loss.code == InventorLossCode::SourceDialectUnverified.kind()
                    || loss.code == InventorLossCode::KernelDialectUnverified.kind()
                    || loss.code == InventorLossCode::KernelCarrierUnparseable.kind()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            inspected_classification, decoded_classification,
            "{}: inspect and decode must report the same classification losses",
            case.label
        );

        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .expect("the synthetic document decodes");
        assert_eq!(
            summary.dialects(),
            decoded.report().dialects(),
            "{}: inspect and decode must report the full layer list",
            case.label
        );
        let source = decoded.ir().source.as_ref().expect("Inventor source meta");
        assert_eq!(source.format(), FORMAT, "{}", case.label);
        assert_eq!(source.dialect(), Some(&matched), "{}", case.label);
    }
}

#[test]
fn inspect_and_decode_do_not_invent_a_kernel_layer_without_kernel_evidence() {
    let bytes = primary_envelope_fixture_with_unavailable_carrier();
    let summary = InventorCodec
        .inspect(
            &mut std::io::Cursor::new(&bytes),
            &InspectOptions::default(),
        )
        .expect("the malformed carrier envelope still inspects");
    let decoded = InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("an unavailable carrier degrades instead of refusing");

    assert_eq!(summary.dialects(), decoded.report().dialects());
    let dialects = decoded.report().dialects();
    let layers = dialects
        .as_ref()
        .expect("Inventor reports its dialect layers");
    assert!(layers
        .iter()
        .all(|matched| matched.format() != cadmpeg_asm::dialect::FORMAT));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| { loss.code != InventorLossCode::KernelDialectUnverified.kind() }));
}

#[test]
fn a_selected_unparseable_kernel_carrier_charges_its_retained_layer() {
    let matched = cadmpeg_asm::dialect::classify(cadmpeg_asm::dialect::KernelHeaderRef::Unknown);
    let loss = kernel_dialect_loss(&matched).expect("refused embedded layer is a reported loss");
    assert_eq!(loss.code, InventorLossCode::KernelCarrierUnparseable.kind());
    assert!(loss.message.contains("native records remain retained"));
}

/// The loss names what diverged, so a reader does not have to re-derive it.
#[test]
fn the_charged_loss_names_the_declaration_that_diverged() {
    let (_, losses) = decoded(&primary_envelope_fixture_with(EnvelopeDeclarations {
        schema: 12,
        meta_marker: "RSe Meta Stream Version 9",
        meta_version: 9,
    }));
    let note = losses
        .iter()
        .find(|loss| loss.code == InventorLossCode::SourceDialectUnverified.kind())
        .expect("an unimplemented declaration charges the dialect loss");
    assert!(note.message.contains("RSe database schema 12"), "{note:?}");
    assert!(
        note.message.contains("RSe Meta Stream Version 9"),
        "{note:?}"
    );
    assert!(note.message.contains("version 9"), "{note:?}");
}

#[test]
fn mixed_unframed_and_foreign_declarations_report_every_admission_cause() {
    let verified_meta = MetaStreamDeclaration {
        marker: MetaStreamDeclaration::VERIFIED_MARKER.to_owned(),
        version: MetaStreamDeclaration::VERIFIED_VERSION,
    };
    let foreign_meta = MetaStreamDeclaration {
        marker: "RSe Meta Stream Version 9".to_owned(),
        version: 9,
    };
    let foreign_schema = RseSchema::from_declared(12);
    let recovery = DialectRecovery {
        cfb_major_version: 3,
        schemas: vec![foreign_schema, RseSchema::SCHEMA_31],
        unframed_schemas: vec![RseSchema::SCHEMA_31],
        meta_streams: vec![verified_meta.clone(), foreign_meta.clone()],
        unframed_meta_streams: vec![verified_meta],
    };

    let classification = recovery.classify();
    let note = classification
        .loss
        .expect("mixed admission failures charge one complete dialect loss");
    assert!(note
        .message
        .contains("schema 31 is declared but its body does not frame"));
    assert!(note.message.contains("database schema 12 is declared"));
    assert!(note.message.contains(
        "marker \"RSe Meta Stream Version 8\" version 8 is declared but its body does not frame"
    ));
    assert!(note
        .message
        .contains("marker \"RSe Meta Stream Version 9\" version 9 is declared"));
}

/// The coverage counts one decode of `bytes` reported.
fn coverage(bytes: &[u8]) -> std::collections::BTreeMap<String, usize> {
    InventorCodec
        .decode(
            &mut std::io::Cursor::new(bytes.to_vec()),
            &DecodeOptions::default(),
        )
        .expect("both version gates degrade rather than refuse")
        .report()
        .coverage
        .clone()
}

#[test]
fn a_foreign_declaration_is_read_with_the_verified_grammar_not_declined() {
    // The unverified label is earned by the declaration, and the streams are
    // still read: every foreign-declaration document covers exactly what the
    // verified one covers, because the schema-31 and version-8 grammars are
    // applied to it rather than withheld. A decline would show up here as an
    // empty registry, empty metadata, or both.
    let verified = coverage(&primary_envelope_fixture_with(
        EnvelopeDeclarations::default(),
    ));
    assert!(verified["rse_registry_entries"] > 0);
    assert!(verified["rse_segment_meta"] > 0);

    for case in CASES {
        if case.admitted || case.declarations.is_none() {
            continue;
        }
        assert_eq!(
            coverage(&case.bytes()),
            verified,
            "{}: the substituted grammar was applied, so the same streams are read",
            case.label
        );
    }
}
