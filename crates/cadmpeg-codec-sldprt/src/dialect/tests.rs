// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.
//!
//! No golden fixture carries a `swSolidWorks` block, so no golden exercises a
//! `swVersion` declaration at all: every frozen `.sldprt` in the tree
//! classifies as `sldprt:unknown` with an empty `declared` map, is
//! `Residual`, and charges `source.dialect-unverified`. The
//! declaration table below carries the versioned-row coverage instead, over
//! containers built by [`crate::test_support::container`].

#![allow(clippy::unwrap_used)]

use super::*;
use crate::container::scan_bytes;
use crate::test_support::{
    make_block, outer_header, sldprt_with_colliding_sites, synthetic_sldprt,
};
use crate::SldprtCodec;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::report::Severity;
use std::collections::BTreeSet;

#[test]
fn enum_and_registry_rows_are_closed_bidirectionally() {
    cadmpeg_test_support::assert_dialect_rows_closed(
        &SldprtDialect::ALL.map(SldprtDialect::id),
        FORMAT,
    );
}

#[test]
fn first_solidworks_envelope_selects_the_written_dialect() {
    let sections = [
        (
            "Contents/Features",
            br#"<?xml version="1.0"?><swSolidWorks swVersion="11000"/>"#.as_slice(),
        ),
        (
            "Contents/SolidWorks",
            br#"<?xml version="1.0"?><swSolidWorks swVersion="34000"/>"#.as_slice(),
        ),
    ];
    let declaration =
        crate::container::first_solidworks_envelope(sections.iter().map(|(_, payload)| *payload))
            .and_then(|envelope| envelope.sw_version);
    let dialect = crate::dialect::SldprtDialect::from_declaration(declaration.as_deref());

    assert_eq!(dialect, crate::dialect::SldprtDialect::SwVersionPre12000);
}

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

#[test]
fn parasolid_schema_evidence_emits_a_kernel_layer() {
    let bytes = synthetic_sldprt();
    let scan = scan_bytes(&bytes);
    let classification = classify_layers(&scan);
    let layers = classification.layers();
    let kernel = layers
        .iter()
        .find(|matched| matched.format() == PARASOLID_FORMAT)
        .expect("the framed Parasolid stream emits a layer");

    assert_eq!(kernel.dialect().as_str(), "parasolid:sch-sw-33103");
    assert_eq!(kernel.declared()["schema"], "SCH_SW_33103_11000");
    assert_eq!(kernel.instance(), None);
    assert_eq!(kernel.admission(), &Admission::Admitted);
}

#[test]
fn residual_parasolid_schema_charges_a_strict_dialect_loss() {
    let host = SldprtDialect::classify(Some("13100"));
    let kernel = cadmpeg_parasolid::classify_layer(
        "SCH_TEST_1_9999",
        "block@7:body+3",
        cadmpeg_core::dialect::LayerInstance::Sole,
        &VERIFIED_KERNELS,
    );
    let layers = DialectLayers::of(host).with(kernel);
    let losses = dialect_losses(&layers);

    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        SldprtLossCode::KernelDialectUnverified.kind()
    );
    assert_eq!(
        losses[0].strict_consequence(),
        cadmpeg_ir::report::StrictConsequence::Reject
    );
    assert!(losses[0].message.contains("SCH_TEST_1_9999"));
    assert!(losses[0].message.contains("block@7:body+3"));
}

#[test]
fn several_parasolid_streams_use_their_source_carriers_as_instances() {
    let bytes = sldprt_with_colliding_sites();
    let scan = scan_bytes(&bytes);
    let kernels = classify_layers(&scan)
        .layers()
        .iter()
        .filter(|matched| matched.format() == PARASOLID_FORMAT)
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(kernels.len(), 2);
    assert!(kernels.iter().all(|matched| {
        matched.instance() == matched.declared().get("carrier").map(String::as_str)
    }));
    assert_ne!(kernels[0].instance(), kernels[1].instance());
}

#[test]
fn duplicate_carrier_identity_is_omitted_with_a_typed_loss() {
    let bytes = sldprt_with_colliding_sites();
    let mut scan = scan_bytes(&bytes);
    scan.blocks.push(scan.blocks[0].clone());

    let classification = classify_layers(&scan);
    let kernel_count = classification
        .layers()
        .iter()
        .filter(|matched| matched.format() == PARASOLID_FORMAT)
        .count();

    assert_eq!(kernel_count, 2);
    let mut losses = Vec::new();
    classification.append_losses(&mut losses);
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == SldprtLossCode::DialectLayerCollision.kind())
            .count(),
        1
    );
}

#[test]
fn inspect_exposes_the_same_host_admission_loss_as_decode() {
    let bytes = synthetic_sldprt();
    let summary = SldprtCodec
        .inspect(&mut std::io::Cursor::new(bytes), &InspectOptions::default())
        .expect("the synthetic part inspects");

    assert!(summary
        .losses
        .iter()
        .any(|loss| loss.code == SldprtLossCode::SourceDialectUnverified.kind()));
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
/// [`SldprtDialect::form_code_padding`] owns the rule over
/// `swVersion.parse::<u32>()`, so the arms are: a parse failure, zero, below
/// 12000, and 12000 or above. 11999 and 12000 pin the boundary.
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

        assert_eq!(matched.dialect().as_str(), case.id, "{context}");
        assert_eq!(matched.format(), FORMAT, "{context}");
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
                    .declared()
                    .get(DECLARED_SW_VERSION)
                    .map(String::as_str),
                Some(declaration),
                "{context}: the declaration is recorded as the source made it"
            ),
            None => assert!(
                matched.declared().is_empty(),
                "{context}: a document that declares nothing declares nothing"
            ),
        }
        assert_eq!(
            matched.declared().len(),
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

    assert_eq!(matched.declared()[DECLARED_SW_VERSION], "SW2019");
    assert_eq!(matched.dialect().as_str(), "sldprt:unknown");
}

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    // The biconditional the decode policy requires. `dialect_loss` reads the
    // admission the report carries rather than reclassifying, so this holds
    // structurally; the test states the contract and fails if either side is
    // ever given its own predicate.
    let charged = SldprtLossCode::SourceDialectUnverified
        .note(String::new())
        .code;

    for case in CASES {
        let matched = SldprtDialect::classify(case.declaration);
        let loss = dialect_loss(&matched);
        let context = format!("swVersion {:?}", case.declaration);

        assert_eq!(
            *matched.admission() == Admission::Admitted,
            loss.is_none(),
            "{context}: admission and the dialect-unverified loss must agree"
        );
        if let Some(note) = &loss {
            assert_eq!(note.code, charged, "{context}");
            assert_eq!(note.severity, Severity::Warning, "{context}");
        }
    }
}

#[test]
fn the_versioned_rows_verify_a_declaration_and_the_residual_row_cannot() {
    // Admission verifies a *declared* identity. A part declaring 11999 or
    // 12000 is read with the padding its own declaration selects, so it is
    // `Admitted`. A part declaring nothing usable has no declaration to verify
    // against, so it is `Residual`: read without any declared grammar. The
    // residual fallback does not claim another row's strategy.
    // The pair (`sldprt:unknown`, `Admitted`) must be unreachable.
    for case in CASES {
        let matched = SldprtDialect::classify(case.declaration);
        let context = format!("swVersion {:?}", case.declaration);
        let residual = matched.dialect().as_str() == SldprtDialect::Unknown.id().as_str();

        let expected = if residual {
            Admission::Residual
        } else {
            Admission::Admitted
        };
        assert_eq!(matched.admission(), &expected, "{context}");
        assert_eq!(
            residual,
            case.id == "sldprt:unknown",
            "{context}: the case table and the classifier disagree on the row"
        );
    }
}

#[test]
fn the_unverified_note_records_the_declaration_that_failed_to_verify() {
    // A declaration the padding rule cannot use is named in the note; an
    // absent one is described as absent. The two are different states and a
    // reader must be able to tell them apart.
    let named = dialect_loss(&SldprtDialect::classify(Some("SW2019")))
        .expect("a non-numeric declaration is unverified");
    assert!(
        named.message.contains("\"SW2019\""),
        "the note must quote the declaration: {}",
        named.message
    );

    let absent =
        dialect_loss(&SldprtDialect::classify(None)).expect("no declaration is unverified");
    assert!(
        absent
            .message
            .contains("no swSolidWorks swVersion declaration"),
        "the note must say the declaration is absent: {}",
        absent.message
    );
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
        [
            SldprtDialect::SwVersionPre12000,
            SldprtDialect::SwVersion12000Plus,
            SldprtDialect::Unknown,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>(),
        "the case table must exercise every row"
    );
}

#[test]
fn the_scan_read_and_the_report_classify_the_same_declaration() {
    // `classify_scan` reads through `container::declared_sw_version`.
    // This checks the report wiring end to end over a real container rather
    // than the identity of two expressions.
    for declaration in ["11999", "12000", "SW2019"] {
        let bytes = container_declaring(declaration);
        let scan = scan_bytes(&bytes);

        assert_eq!(
            crate::container::declared_sw_version(&scan),
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

    assert_eq!(crate::container::declared_sw_version(&scan), None);
    assert_eq!(matched.dialect().as_str(), "sldprt:unknown");
    assert!(matched.declared().is_empty());
    assert_eq!(matched.admission(), &Admission::Residual);
    assert!(dialect_loss(&matched).is_some());
}

#[test]
fn exactly_one_entry_names_the_reporting_format() {
    let bytes = container_declaring("13100");
    let scan = scan_bytes(&bytes);
    let dialects = [SldprtDialect::classify_scan(&scan)];

    assert_eq!(dialects.len(), 1);
    assert_eq!(
        dialects
            .iter()
            .find(|entry| entry.format() == FORMAT)
            .map(cadmpeg_core::dialect::DialectMatch::dialect),
        Some(&SldprtDialect::SwVersion12000Plus.id())
    );
}
