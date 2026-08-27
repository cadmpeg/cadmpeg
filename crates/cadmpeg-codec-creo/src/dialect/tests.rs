// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.
//!
//! `creo:legacy-ascii` has no golden fixture — the committed set splits across
//! ND, DEPDB, and unknown — so the synthetic frames below are that row's only
//! evidence that classification reaches it at all.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::container::scan_bytes;
use crate::test_support::{build_prt, build_prt_raw};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Path of the identity registry, from this crate's manifest directory.
fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/dialects.toml")
        .canonicalize()
        .expect("docs/dialects.toml resolves from the crate manifest directory")
}

/// Every `id = "creo:…"` value in `docs/dialects.toml`.
fn registry_ids() -> BTreeSet<String> {
    let text = std::fs::read_to_string(registry_path()).expect("read docs/dialects.toml");
    let ids = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("id = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|id| id.starts_with("creo:"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no creo rows");
    ids
}

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    let pinned = Layout::ALL
        .iter()
        .map(|layout| layout.id().as_str().to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        pinned.len(),
        Layout::ALL.len(),
        "two variants pin the same id"
    );
    assert_eq!(
        pinned,
        registry_ids(),
        "docs/dialects.toml and Layout disagree; ids are pinned forever, so reconcile the enum"
    );
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

/// Containers spanning every arm of `identify_layout`, including both causes
/// that collapse into `Layout::Unknown`.
const CASES: &[Case] = &[
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
        layout: Layout::LegacyAscii,
        id: "creo:legacy-ascii",
        admitted: true,
        legacy_schema: Some("12"),
        legacy_release: Some("16.0"),
    },
    Case {
        label: "legacy ASCII frame with no release word",
        bytes: legacy_ascii_without_release_bytes,
        layout: Layout::LegacyAscii,
        id: "creo:legacy-ascii",
        admitted: true,
        legacy_schema: Some("12"),
        legacy_release: None,
    },
    Case {
        label: "DEPDB_DATA without the root record",
        bytes: depdb_without_root_bytes,
        layout: Layout::Unknown,
        id: "creo:unknown",
        admitted: false,
        legacy_schema: None,
        legacy_release: None,
    },
    Case {
        label: "no layout signature at all",
        bytes: unknown_bytes,
        layout: Layout::Unknown,
        id: "creo:unknown",
        admitted: false,
        legacy_schema: None,
        legacy_release: None,
    },
];

#[test]
fn each_container_classifies_into_the_row_its_discriminants_match() {
    for case in CASES {
        let bytes = (case.bytes)();
        let scan = scan_bytes(bytes.as_slice());
        assert_eq!(scan.framing.layout, case.layout, "{}", case.label);

        let matched = classify(&scan);
        assert_eq!(matched.format, FORMAT, "{}", case.label);
        assert_eq!(
            matched.dialect.as_ref().map(DialectId::as_str),
            Some(case.id),
            "{}",
            case.label
        );
        assert_eq!(
            matched.declared[DECLARED_VERSION_LINE], scan.framing.version_line,
            "{}: the header line is recorded as the source wrote it",
            case.label
        );
        assert_eq!(
            matched
                .declared
                .get(DECLARED_LEGACY_ASCII_SCHEMA)
                .map(String::as_str),
            case.legacy_schema,
            "{}",
            case.label
        );
        assert_eq!(
            matched
                .declared
                .get(DECLARED_LEGACY_ASCII_PRODUCT_RELEASE)
                .map(String::as_str),
            case.legacy_release,
            "{}",
            case.label
        );

        let expected_admission = if case.admitted {
            Admission::Admitted
        } else {
            Admission::AdmittedUnverified {
                nearest: DialectId::pinned("creo:unknown"),
            }
        };
        assert_eq!(matched.admission, expected_admission, "{}", case.label);
    }
}

#[test]
fn admission_is_admitted_exactly_when_no_dialect_unverified_loss_is_charged() {
    // Over the closed `Layout` enum, not only over the synthetic containers:
    // the biconditional is a property of the predicate, and both facts read it.
    for layout in [
        Layout::Nd,
        Layout::Depdb,
        Layout::LegacyAscii,
        Layout::Unknown,
    ] {
        let charged = dialect_loss(layout).is_some();
        assert_eq!(
            layout_is_declared(layout),
            !charged,
            "{layout:?}: the recovery predicate and the charged loss disagree"
        );
    }

    for case in CASES {
        let bytes = (case.bytes)();
        let scan = scan_bytes(bytes.as_slice());
        let charged = dialect_loss(scan.framing.layout).is_some();
        assert_eq!(
            classify(&scan).admission == Admission::Admitted,
            !charged,
            "{}: admission and the dialect-unverified loss must agree",
            case.label
        );
    }
}

#[test]
fn the_dialect_unverified_loss_carries_the_shared_taxonomy() {
    let note = dialect_loss(Layout::Unknown).expect("an unclassified layout charges the loss");
    assert_eq!(note.code.as_str(), "creo/source.dialect-unverified");
    assert_eq!(
        note.code.taxonomy(),
        cadmpeg_ir::report::LossTaxonomy::SourceDialectUnverified
    );
    // The shared taxonomy carries a strict floor of `Warning`, so
    // `DecodeMode::Strict` now means "classified layouts only" for Creo.
    assert_eq!(
        note.code.strict_floor(),
        Some(cadmpeg_ir::report::Severity::Warning)
    );
}

#[test]
fn the_totality_row_never_carries_a_verified_admission() {
    // `creo:unknown` states that no layout discriminant matched. A document
    // there necessarily skipped every layout-specific decode gate, so the pair
    // (unknown, Admitted) must be unreachable.
    for case in CASES {
        let bytes = (case.bytes)();
        let scan = scan_bytes(bytes.as_slice());
        let matched = classify(&scan);
        if matched.dialect.as_ref().map(DialectId::as_str) == Some(Layout::Unknown.id().as_str()) {
            assert_ne!(matched.admission, Admission::Admitted, "{}", case.label);
        }
    }
}

#[test]
fn the_layout_token_vocabulary_is_not_the_registry_vocabulary() {
    // The `layout` source attribute and the inspect note are pinned on
    // `Layout::token`; the registry ids are pinned here. They are separate
    // contracts and neither may be derived from the other by string surgery.
    for layout in [
        Layout::Nd,
        Layout::Depdb,
        Layout::LegacyAscii,
        Layout::Unknown,
    ] {
        let id = layout.id();
        assert_ne!(id.as_str(), layout.token());
        assert!(id.as_str().starts_with("creo:"));
    }
}
