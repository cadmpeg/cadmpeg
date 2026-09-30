// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.
//!
//! `creo:legacy-ascii` has no golden fixture — the committed set splits across
//! ND, DEPDB, and unknown — so the synthetic frames below are that row's only
//! evidence that classification reaches it at all.

#![allow(clippy::unwrap_used)]

use super::{
    classify, DECLARED_LEGACY_ASCII_PRODUCT_RELEASE, DECLARED_LEGACY_ASCII_SCHEMA,
    DECLARED_VERSION_LINE, FORMAT,
};
use crate::container::scan_bytes_ok;
use crate::container::{Layout, UnknownLayout};
use crate::test_support::{build_prt, build_prt_raw};
use cadmpeg_core::dialect::Admission;

fn classify_ok(scan: &crate::container::ContainerScan<'_>) -> super::DialectClassification {
    crate::decode::with_test_decode_ctx(|ctx| classify(ctx, scan))
        .expect("dialect declaration is admitted")
}

fn loss_ok(
    classification: &super::DialectClassification,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    crate::decode::with_test_decode_ctx(|ctx| classification.loss(ctx))
        .expect("dialect loss is admitted")
}

fn legacy_classification_with_limits(
    collection_limit: u64,
    retained_limit: u64,
) -> Result<super::DialectClassification, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let bytes = legacy_ascii_bytes();
    let scan = scan_bytes_ok(bytes.as_slice());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
    classify(&ctx, &scan)
}

#[test]
fn legacy_declared_nodes_charge_each_key_before_insertion() {
    for limit in 0..3 {
        let Err(error) = legacy_classification_with_limits(limit, u64::MAX) else {
            panic!("each of three declared fields needs one node")
        };
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && refusal.operation == "creo declared dialect nodes"
        ));
    }
    assert!(legacy_classification_with_limits(3, u64::MAX).is_ok());
}

fn assert_legacy_retained_boundary(operation: &'static str) {
    let found = (0..1024).any(|limit| {
        matches!(
            legacy_classification_with_limits(u64::MAX, limit),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && refusal.operation == operation
        )
    });
    assert!(found, "{operation} refuses immediately below its text need");
}

#[test]
fn legacy_declared_schema_refuses_retained_limit() {
    assert_legacy_retained_boundary("creo declared legacy schema");
}

#[test]
fn legacy_declared_release_refuses_retained_limit() {
    assert_legacy_retained_boundary("creo declared product release");
}

#[test]
fn enum_and_registry_rows_are_closed_bidirectionally() -> Result<(), Box<dyn std::error::Error>> {
    cadmpeg_test_support::assert_dialect_rows_closed(
        &[
            Layout::Nd,
            Layout::Depdb,
            crate::test_support::legacy_layout(),
            Layout::Unknown(UnknownLayout::NoDiscriminant),
        ]
        .map(|layout| layout.id()),
        FORMAT,
    )?;
    Ok(())
}

/// A PSB file whose only section carries the `ND:` raw-name decoration.
fn nd_bytes() -> Vec<u8> {
    build_prt("c", &[("ND:0:VisibGeom", b"payload".to_vec())])
}

/// A PSB file with a `DEPDB_DATA` section; `build_prt` prefixes the root record.
fn depdb_bytes() -> Vec<u8> {
    build_prt("c", &[("DEPDB_DATA", b"payload".to_vec())])
}

/// A PSB file with a `DEPDB_DATA` section whose payload is not the root record.
///
/// The hard-unknown cause: the DEPDB test is exclusive, so this does not fall
/// through to the ND or legacy ASCII tests.
fn depdb_without_root_bytes() -> Vec<u8> {
    build_prt_raw("c", &[("DEPDB_DATA", b"not the root record".to_vec())])
}

/// A malformed DEPDB discriminator alongside an ND-decorated section.
///
/// DEPDB presence is exclusive, so the invalid root keeps this in the unknown
/// row even though an ND decoration exists.
fn depdb_without_root_and_nd_bytes() -> Vec<u8> {
    build_prt_raw(
        "c",
        &[
            ("DEPDB_DATA", b"not the root record".to_vec()),
            ("ND:0:VisibGeom", b"payload".to_vec()),
        ],
    )
}

/// A PSB file matching none of the three signatures.
fn unknown_bytes() -> Vec<u8> {
    build_prt("c", &[("VisibGeom", b"payload".to_vec())])
}

/// A complete legacy ASCII `P_OBJECT` frame with a `Release` banner.
fn legacy_ascii_bytes() -> Vec<u8> {
    b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 12\n@root 1 0\n0 1 7\n\
      #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Release 16.0  All Rights Reserved\n"
        .to_vec()
}

/// The same frame with a banner declaring no `Version` or `Release` word.
fn legacy_ascii_without_release_bytes() -> Vec<u8> {
    b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 12\n@root 1 0\n0 1 7\n\
      #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  All Rights Reserved\n"
        .to_vec()
}

/// One matrix row: a synthetic container and the row it must classify into.
struct Case {
    /// What the bytes are, for failure messages.
    label: &'static str,
    /// Builder for the container bytes.
    bytes: fn() -> Vec<u8>,
    /// The layout `identify_layout` must reach.
    layout: Layout,
    /// The registry id the match must carry.
    id: &'static str,
    /// Whether the layout named a dialect whose own strategy was applied.
    admitted: bool,
    /// The `legacy_ascii_schema` declaration, when the frame declares one.
    legacy_schema: Option<&'static str>,
    /// The `legacy_ascii_product_release` declaration, when the banner has one.
    legacy_release: Option<&'static str>,
}

/// Containers spanning every arm of `identify_layout`, including both unknown causes.
fn cases() -> [Case; 7] {
    [
        Case {
            label: "ND: decorated section",
            bytes: nd_bytes,
            layout: Layout::Nd,
            id: "creo:nd",
            admitted: true,
            legacy_schema: None,
            legacy_release: None,
        },
        Case {
            label: "DEPDB_DATA with the root record",
            bytes: depdb_bytes,
            layout: Layout::Depdb,
            id: "creo:depdb",
            admitted: true,
            legacy_schema: None,
            legacy_release: None,
        },
        Case {
            label: "legacy ASCII frame with a Release banner",
            bytes: legacy_ascii_bytes,
            layout: crate::test_support::legacy_layout(),
            id: "creo:legacy-ascii",
            admitted: true,
            legacy_schema: Some("12"),
            legacy_release: Some("16.0"),
        },
        Case {
            label: "legacy ASCII frame with no release word",
            bytes: legacy_ascii_without_release_bytes,
            layout: crate::test_support::legacy_layout(),
            id: "creo:legacy-ascii",
            admitted: true,
            legacy_schema: Some("12"),
            legacy_release: None,
        },
        Case {
            label: "DEPDB_DATA without the root record",
            bytes: depdb_without_root_bytes,
            layout: Layout::Unknown(UnknownLayout::DepdbRootMissing),
            id: "creo:unknown",
            admitted: false,
            legacy_schema: None,
            legacy_release: None,
        },
        Case {
            label: "DEPDB_DATA without the root record plus ND decoration",
            bytes: depdb_without_root_and_nd_bytes,
            layout: Layout::Unknown(UnknownLayout::DepdbRootMissing),
            id: "creo:unknown",
            admitted: false,
            legacy_schema: None,
            legacy_release: None,
        },
        Case {
            label: "no layout signature at all",
            bytes: unknown_bytes,
            layout: Layout::Unknown(UnknownLayout::NoDiscriminant),
            id: "creo:unknown",
            admitted: false,
            legacy_schema: None,
            legacy_release: None,
        },
    ]
}

#[test]
fn each_container_classifies_into_the_row_its_discriminants_match() {
    for case in cases() {
        let bytes = (case.bytes)();
        let scan = scan_bytes_ok(bytes.as_slice());
        match &case.layout {
            Layout::LegacyAscii(_) => assert!(
                matches!(scan.framing.layout, Layout::LegacyAscii(_)),
                "{}",
                case.label
            ),
            _ => assert_eq!(scan.framing.layout, case.layout, "{}", case.label),
        }

        let classification = classify_ok(&scan);
        let matched = classification.matched();
        assert_eq!(matched.format(), FORMAT, "{}", case.label);
        assert_eq!(matched.dialect().as_str(), case.id, "{}", case.label);
        assert_eq!(
            matched.declared()[DECLARED_VERSION_LINE],
            scan.framing.version_line,
            "{}: the header line is recorded as the source wrote it",
            case.label
        );
        assert_eq!(
            matched
                .declared()
                .get(DECLARED_LEGACY_ASCII_SCHEMA)
                .map(String::as_str),
            case.legacy_schema,
            "{}",
            case.label
        );
        assert_eq!(
            matched
                .declared()
                .get(DECLARED_LEGACY_ASCII_PRODUCT_RELEASE)
                .map(String::as_str),
            case.legacy_release,
            "{}",
            case.label
        );

        let expected_admission = if case.admitted {
            Admission::Admitted
        } else {
            Admission::Residual
        };
        assert_eq!(*matched.admission(), expected_admission, "{}", case.label);
    }
}

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    // Over the closed `Layout` enum, not only over the synthetic containers:
    // the biconditional is a property of the predicate, and both facts read it.
    for layout in [
        Layout::Nd,
        Layout::Depdb,
        crate::test_support::legacy_layout(),
        Layout::Unknown(UnknownLayout::NoDiscriminant),
    ] {
        let bytes = match layout {
            Layout::Nd => nd_bytes(),
            Layout::Depdb => depdb_bytes(),
            Layout::LegacyAscii(_) => legacy_ascii_bytes(),
            Layout::Unknown(_) => unknown_bytes(),
        };
        let scan = scan_bytes_ok(bytes.as_slice());
        let classification = classify_ok(&scan);
        let matched = classification.matched();
        let charged = loss_ok(&classification).is_some();
        assert_eq!(*matched.admission() == Admission::Admitted, !charged);
    }

    for case in cases() {
        let bytes = (case.bytes)();
        let scan = scan_bytes_ok(bytes.as_slice());
        let classification = classify_ok(&scan);
        let matched = classification.matched();
        let charged = loss_ok(&classification).is_some();
        assert_eq!(
            *matched.admission() == Admission::Admitted,
            !charged,
            "{}: admission and the dialect-unverified loss must agree",
            case.label
        );
    }
}

#[test]
fn the_dialect_unverified_loss_carries_the_shared_taxonomy() {
    let bytes = unknown_bytes();
    let scan = scan_bytes_ok(bytes.as_slice());
    let note = loss_ok(&classify_ok(&scan)).expect("an unclassified layout charges the loss");
    assert_eq!(note.code.to_string(), "creo/source.dialect-unverified");
    assert_eq!(
        note.code.taxonomy(),
        cadmpeg_ir::report::loss::LossTaxonomy::SourceDialectUnverified
    );
    // The shared taxonomy carries a strict floor of `Warning`, so
    // `DecodeMode::Strict` now means "classified layouts only" for Creo.
    assert_eq!(
        note.code.strict_floor(),
        Some(cadmpeg_ir::report::Severity::Warning)
    );
}

#[test]
fn malformed_depdb_loss_does_not_deny_present_nd_evidence() {
    let bytes = depdb_without_root_and_nd_bytes();
    let scan = scan_bytes_ok(bytes.as_slice());
    let note = loss_ok(&classify_ok(&scan)).expect("unknown layout loss");

    assert!(note.message.contains("DEPDB_DATA is the exclusive"));
    assert!(!note.message.contains("no ND:"));
}

#[test]
fn the_totality_row_never_carries_a_verified_admission() {
    // `creo:unknown` states that no layout discriminant matched. A document
    // there necessarily skipped every layout-specific decode gate, so the pair
    // (unknown, Admitted) must be unreachable.
    for case in cases() {
        let bytes = (case.bytes)();
        let scan = scan_bytes_ok(bytes.as_slice());
        let classification = classify_ok(&scan);
        let matched = classification.matched();
        if matched.dialect().as_str()
            == Layout::Unknown(UnknownLayout::NoDiscriminant).id().as_str()
        {
            assert_ne!(*matched.admission(), Admission::Admitted, "{}", case.label);
        }
    }
}

#[test]
fn the_layout_token_vocabulary_is_not_the_registry_vocabulary() {
    // The inspect note is pinned on `Layout::token`; the registry ids are
    // pinned here. They are separate contracts and neither may be derived from
    // the other by string surgery.
    for layout in [
        Layout::Nd,
        Layout::Depdb,
        crate::test_support::legacy_layout(),
        Layout::Unknown(UnknownLayout::NoDiscriminant),
    ] {
        let id = layout.id();
        assert_ne!(id.as_str(), layout.token());
        assert!(id.as_str().starts_with("creo:"));
    }
}

#[test]
fn source_dialect_copy_refuses_each_declaration_node() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let bytes = legacy_ascii_bytes();
    let scan = scan_bytes_ok(bytes.as_slice());
    let classification = classify_ok(&scan);
    for cap in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let error = classification
            .copy_matched_admitted(&ctx)
            .expect_err("declaration node");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "creo source dialect declaration nodes")
        );
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| classification.copy_matched_admitted(ctx))
            .expect("service"),
        *classification.matched()
    );
}

#[test]
fn source_dialect_copy_refuses_keys_and_values_and_preserves_all_layouts() {
    for case in cases() {
        let bytes = (case.bytes)();
        let scan = scan_bytes_ok(bytes.as_slice());
        let classification = classify_ok(&scan);
        let copied = crate::test_support::assert_retained_boundaries(
            &[
                "creo source dialect declaration key",
                "creo source dialect declaration value",
            ],
            |ctx| classification.copy_matched_admitted(ctx),
        );
        assert_eq!(copied, *classification.matched(), "{}", case.label);
    }
}
