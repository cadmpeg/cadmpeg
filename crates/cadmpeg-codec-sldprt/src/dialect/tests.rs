// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.
//!
//! No golden fixture carries a `swSolidWorks` block, so no golden exercises a
//! `swVersion` declaration at all: every frozen `.sldprt` in the tree
//! classifies as `sldprt:unknown` with an empty `declared` map. The declaration
//! table below carries that coverage instead, over containers built by
//! [`crate::test_support::container`].

#![allow(clippy::unwrap_used)]

use super::*;
use crate::container::scan_bytes;
use crate::test_support::{make_block, outer_header};
use cadmpeg_core::dialect::primary_layer;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// A synthetic `.sldprt` whose `Contents/SolidWorks` block declares
/// `swVersion`, verbatim as given.
///
/// `test_support::container::add_solidworks_version` takes a `u32`, so it
/// cannot express the declarations the padding rule rejects. Those are the
/// interesting half of the table.
fn container_declaring(sw_version: &str) -> Vec<u8> {
    let mut bytes = outer_header();
    bytes.extend(make_block(
        0x43,
        "Contents/SolidWorks",
        format!(r#"<?xml version="1.0"?><swSolidWorks swVersion="{sw_version}"/>"#).as_bytes(),
    ));
    bytes
}

/// Path of the identity registry, from this crate's manifest directory.
fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/dialects.toml")
        .canonicalize()
        .expect("docs/dialects.toml resolves from the crate manifest directory")
}

/// Every `id = "sldprt:…"` value in `docs/dialects.toml`.
fn registry_ids() -> BTreeSet<String> {
    let text = std::fs::read_to_string(registry_path()).expect("read docs/dialects.toml");
    let ids = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("id = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|id| id.starts_with("sldprt:"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no sldprt rows");
    ids
}

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    let pinned = SldprtDialect::ALL
        .iter()
        .map(|dialect| dialect.id().as_str().to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        pinned.len(),
        SldprtDialect::ALL.len(),
        "two variants pin the same id"
    );
    assert_eq!(
        pinned,
        registry_ids(),
        "docs/dialects.toml and SldprtDialect disagree; ids are pinned forever, so reconcile the enum"
    );
}

/// One `swVersion` declaration and the row it selects.
struct Case {
    /// The attribute text, or `None` when the document declares nothing.
    declaration: Option<&'static str>,
    /// Registry id the declaration classifies into.
    id: &'static str,
}

/// Declarations spanning every arm of the padding rule.
///
/// The rule is `(version > 0).then_some(if version >= 12_000 { Eight } else
/// { Four })` over `swVersion.parse::<u32>()`
/// (`resolved_features/operations.rs`), so the arms are: a parse failure, zero,
/// below 12000, and 12000 or above. 11999 and 12000 pin the boundary.
const CASES: &[Case] = &[
    Case {
        declaration: None,
        id: "sldprt:unknown",
    },
    Case {
        declaration: Some(""),
        id: "sldprt:unknown",
    },
    Case {
        declaration: Some("0"),
        id: "sldprt:unknown",
    },
    Case {
        declaration: Some("SW2019"),
        id: "sldprt:unknown",
    },
    Case {
        declaration: Some("-1"),
        id: "sldprt:unknown",
    },
    Case {
        declaration: Some("4294967296"),
        id: "sldprt:unknown",
    },
    Case {
        declaration: Some("1"),
        id: "sldprt:sw-version-pre-12000",
    },
    Case {
        declaration: Some("11999"),
        id: "sldprt:sw-version-pre-12000",
    },
    Case {
        declaration: Some("12000"),
        id: "sldprt:sw-version-12000-plus",
    },
    Case {
        declaration: Some("13100"),
        id: "sldprt:sw-version-12000-plus",
    },
];

#[test]
fn each_declaration_classifies_into_the_row_its_discriminant_matches() {
    for case in CASES {
        let matched = SldprtDialect::classify(case.declaration);
        let context = format!("swVersion {:?}", case.declaration);

        assert_eq!(
            matched.dialect.as_ref().map(DialectId::as_str),
            Some(case.id),
            "{context}"
        );
        assert_eq!(matched.format, FORMAT, "{context}");
    }
}

#[test]
fn the_declaration_is_recorded_verbatim_and_only_when_the_source_makes_one() {
    for case in CASES {
        let matched = SldprtDialect::classify(case.declaration);
        let context = format!("swVersion {:?}", case.declaration);

        match case.declaration {
            Some(declaration) => assert_eq!(
                matched
                    .declared
                    .get(DECLARED_SW_VERSION)
                    .map(String::as_str),
                Some(declaration),
                "{context}: the declaration is recorded as the source made it"
            ),
            None => assert!(
                matched.declared.is_empty(),
                "{context}: a document that declares nothing declares nothing"
            ),
        }
        assert_eq!(
            matched.declared.len(),
            usize::from(case.declaration.is_some()),
            "{context}: sw_version is the only declared key"
        );
    }
}

#[test]
fn the_identity_row_never_reads_the_declaration_a_second_time() {
    // Identity and declaration are different statements. `SW2019` is a
    // SolidWorks release name and is recorded as one, and it classifies as
    // `sldprt:unknown` because the row's discriminant is a usable numeric
    // declaration. A consumer that joins the two gets the wrong answer for
    // exactly the files whose declarations are wrong.
    let matched = SldprtDialect::classify(Some("SW2019"));

    assert_eq!(matched.declared[DECLARED_SW_VERSION], "SW2019");
    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("sldprt:unknown")
    );
}

#[test]
fn admission_is_admitted_on_every_row_because_each_row_runs_its_own_strategy() {
    // There is no dialect-unverified loss in this codec and nothing to name as
    // `nearest`: the pre-12000 row narrows the form-code padding filter to four
    // bytes, the 12000-plus row to eight, and `sldprt:unknown` does not apply
    // the filter at all and requires the two candidate offsets to agree. None
    // of the three substitutes another row's grammar. If a future change makes
    // one row read with another's strategy, this test is the one that has to
    // change, and the `SourceDialectUnverified` charge lands with it.
    for case in CASES {
        assert_eq!(
            SldprtDialect::classify(case.declaration).admission,
            Admission::Admitted,
            "swVersion {:?}",
            case.declaration
        );
    }
}

#[test]
fn classification_is_total_over_the_padding_rule() {
    // B4: every outcome of the discriminant reaches a declared row, and the
    // rows are disjoint. `from_declaration` is a total function of
    // `form_code_padding`, whose three outcomes are the three rows.
    let reached = CASES
        .iter()
        .map(|case| SldprtDialect::from_declaration(case.declaration))
        .collect::<BTreeSet<_>>();

    assert_eq!(
        reached,
        SldprtDialect::ALL.iter().copied().collect::<BTreeSet<_>>(),
        "the case table must exercise every row"
    );
}

#[test]
fn the_scan_read_and_the_attribute_read_classify_the_same_declaration() {
    // `classify_scan` reads through `decode::declared_sw_version` and
    // `source_meta` reads `attributes["sw_version"]`. Both are the output of
    // `add_solidworks_xml_metadata`, so the report entry and the `SourceMeta`
    // mirror cannot disagree. This checks the wiring end to end over a real
    // container rather than the identity of the two expressions.
    for declaration in ["11999", "12000", "SW2019"] {
        let bytes = container_declaring(declaration);
        let scan = scan_bytes(&bytes);

        assert_eq!(
            crate::decode::declared_sw_version(&scan).as_deref(),
            Some(declaration),
            "swVersion {declaration:?} must survive the scan"
        );
        assert_eq!(
            SldprtDialect::classify_scan(&scan),
            SldprtDialect::classify(Some(declaration)),
            "swVersion {declaration:?}"
        );
    }
}

#[test]
fn a_container_declaring_nothing_reaches_the_totality_row() {
    let bytes = outer_header();
    let scan = scan_bytes(&bytes);
    let matched = SldprtDialect::classify_scan(&scan);

    assert_eq!(crate::decode::declared_sw_version(&scan), None);
    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("sldprt:unknown")
    );
    assert!(matched.declared.is_empty());
    assert_eq!(matched.admission, Admission::Admitted);
}

#[test]
fn exactly_one_entry_names_the_reporting_format() {
    let bytes = container_declaring("13100");
    let scan = scan_bytes(&bytes);
    let dialects = vec![SldprtDialect::classify_scan(&scan)];

    assert_eq!(dialects.len(), 1);
    assert_eq!(
        primary_layer(&dialects, FORMAT).and_then(|entry| entry.dialect.as_ref()),
        Some(&SldprtDialect::SwVersion12000Plus.id())
    );
}
