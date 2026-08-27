// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.
//!
//! No `.ipt` or `.iam` file exists in this repository, so every path below is
//! driven from a synthetic document built by [`crate::test_support`]. The
//! declarations are the only thing that varies between them.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::LossNote;

use super::*;
use crate::test_support::{fixture, primary_envelope_fixture_with, EnvelopeDeclarations};
use crate::InventorCodec;

/// Path of the identity registry, from this crate's manifest directory.
fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/dialects.toml")
        .canonicalize()
        .expect("docs/dialects.toml resolves from the crate manifest directory")
}

/// Every `id = "inventor:…"` value in `docs/dialects.toml`.
fn registry_ids() -> BTreeSet<String> {
    let text = std::fs::read_to_string(registry_path()).expect("read docs/dialects.toml");
    let ids = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("id = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|id| id.starts_with("inventor:"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no inventor rows");
    ids
}

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    let pinned = InventorDialect::ALL
        .iter()
        .map(|dialect| dialect.id().as_str().to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        pinned.len(),
        InventorDialect::ALL.len(),
        "two variants pin the same id"
    );
    assert_eq!(
        pinned,
        registry_ids(),
        "docs/dialects.toml and InventorDialect disagree; ids are pinned forever, so reconcile the enum"
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
    let primary = cadmpeg_core::dialect::primary_layer(&report.dialects, FORMAT)
        .unwrap_or_else(|| panic!("one primary layer, got {:#?}", report.dialects));
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
            matched.admission == Admission::Admitted,
            !charged,
            "{}: admission and the dialect-unverified loss must agree",
            case.label
        );
    }
}

#[test]
fn each_document_classifies_into_the_row_its_declarations_match() {
    for case in CASES {
        let (matched, _) = decoded(&case.bytes());
        assert_eq!(
            matched.dialect.as_ref().map(DialectId::as_str),
            Some(case.id),
            "{}",
            case.label
        );

        let expected_admission = if case.admitted {
            Admission::Admitted
        } else {
            Admission::AdmittedUnverified {
                nearest: InventorDialect::Cfb3Rse31Meta8.id(),
            }
        };
        assert_eq!(matched.admission, expected_admission, "{}", case.label);

        assert_eq!(
            matched.declared[DECLARED_CFB_MAJOR_VERSION], "3",
            "{}: the CFB major version is recorded, never gated on",
            case.label
        );
        assert_eq!(
            matched
                .declared
                .get(DECLARED_RSE_DB_SCHEMA)
                .map(String::as_str),
            case.schema,
            "{}: the schema is recorded as the stream declared it",
            case.label
        );
        assert_eq!(
            matched
                .declared
                .get(DECLARED_META_STREAM_VERSION)
                .map(String::as_str),
            case.meta_version,
            "{}: the metadata version is recorded as the stream declared it",
            case.label
        );
        assert_eq!(
            matched
                .declared
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
        if matched.dialect.as_ref().map(DialectId::as_str)
            == Some(InventorDialect::Unknown.id().as_str())
        {
            assert_ne!(matched.admission, Admission::Admitted, "{}", case.label);
        }
    }
}

#[test]
fn inspect_and_decode_report_the_same_match_and_the_source_mirrors_it() {
    for case in CASES {
        let bytes = case.bytes();
        let (matched, _) = decoded(&bytes);

        let summary = InventorCodec
            .inspect(
                &mut std::io::Cursor::new(&bytes),
                &InspectOptions::default(),
            )
            .expect("the synthetic document inspects");
        assert_eq!(summary.dialects.len(), 1, "{:#?}", summary.dialects);
        assert_eq!(summary.dialects[0], matched, "{}", case.label);

        let decoded = InventorCodec
            .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
            .expect("the synthetic document decodes");
        let source = decoded.ir().source.as_ref().expect("Inventor source meta");
        assert_eq!(source.format, FORMAT, "{}", case.label);
        assert_eq!(source.dialect, matched.dialect, "{}", case.label);
        assert_eq!(source.declared, matched.declared, "{}", case.label);
    }
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
