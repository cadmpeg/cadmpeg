// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::*;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Path of the identity registry, from this crate's manifest directory.
fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/dialects.toml")
        .canonicalize()
        .expect("docs/dialects.toml resolves from the crate manifest directory")
}

/// Every `id = "nx:…"` value in `docs/dialects.toml`.
fn registry_ids() -> BTreeSet<String> {
    let text = std::fs::read_to_string(registry_path()).expect("read docs/dialects.toml");
    let ids = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("id = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|id| id.starts_with("nx:"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no nx rows");
    ids
}

/// A container carrying nothing but the dispatch flag and the version byte.
///
/// Classification reads exactly these two fields, so an empty directory is a
/// complete input for it.
fn container(legacy_cfb: bool, version: u8) -> Container<'static> {
    Container {
        data: (&[] as &[u8]).into(),
        version,
        file_tag: 0,
        footer_offset: 0,
        header_entry_count: 0,
        footer_entry_count: 0,
        footer_fingerprint: [0; 4],
        physical_size: 0,
        legacy_cfb,
        entries: Vec::new(),
        indexed_section_layouts: OnceLock::new(),
        om_operation_label_layouts: OnceLock::new(),
        om_section_cache: OnceLock::new(),
    }
}

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    let pinned = NxDialect::ALL
        .iter()
        .map(|dialect| dialect.id().as_str().to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        pinned.len(),
        NxDialect::ALL.len(),
        "two variants pin the same id"
    );
    assert_eq!(
        pinned,
        registry_ids(),
        "docs/dialects.toml and NxDialect disagree; ids are pinned forever, so reconcile the enum"
    );
}

#[test]
fn each_container_parser_classifies_into_its_own_row() {
    assert_eq!(
        NxDialect::of_container(&container(false, 0x06)),
        NxDialect::Splmsstr
    );
    assert_eq!(
        NxDialect::of_container(&container(true, 0x0a)),
        NxDialect::LegacyCfb
    );
}

#[test]
fn the_container_kind_label_and_the_registry_id_come_from_one_enum() {
    assert_eq!(NxDialect::Splmsstr.container_kind(), "splmsstr");
    assert_eq!(NxDialect::Splmsstr.id().as_str(), "nx:splmsstr");
    assert_eq!(NxDialect::LegacyCfb.container_kind(), "cfb");
    assert_eq!(NxDialect::LegacyCfb.id().as_str(), "nx:legacy-cfb");
}

#[test]
fn both_container_rows_are_admitted_because_classification_is_structural() {
    // The dispatch at `decode::scan` picks the parser that then defines the
    // row, so a document is never read with a grammar its row does not
    // declare. No arm of this codec substitutes one row's strategy for
    // another's, so no unverified admission and no dialect-unverified loss
    // exist. Anything else here would be an invented path.
    for legacy_cfb in [false, true] {
        let matched = NxDialect::classify(&container(legacy_cfb, 0x06));
        assert_eq!(matched.admission, Admission::Admitted);
    }
    assert_eq!(NxDialect::Splmsstr.admission(), Admission::Admitted);
    assert_eq!(NxDialect::LegacyCfb.admission(), Admission::Admitted);
}

#[test]
fn the_totality_row_is_declared_but_never_classified() {
    // `nx:unknown` is the B4 row for a file matching neither container. Such a
    // file is refused at the container boundary — `detect` reports
    // `Confidence::No` and a forced scan returns `WrongFormat` — so no run
    // produces a match carrying it. Its declared disposition is refusal.
    assert_eq!(NxDialect::Unknown.admission(), Admission::Refused);
    for legacy_cfb in [false, true] {
        assert_ne!(
            NxDialect::of_container(&container(legacy_cfb, 0)),
            NxDialect::Unknown
        );
    }
}

#[test]
fn the_modern_arm_declares_the_container_version_byte_verbatim() {
    let matched = NxDialect::classify(&container(false, 0x06));

    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("nx:splmsstr")
    );
    assert_eq!(matched.format, "nx");
    assert_eq!(matched.declared[DECLARED_SPLMSSTR_VERSION], "6");
    assert!(!matched.declared.contains_key(DECLARED_UGII_VERSION));
    // No indexed store section, so no `store_version` record is available.
    assert!(!matched.declared.contains_key(DECLARED_PRODUCT_VERSION));
}

#[test]
fn the_legacy_arm_declares_the_ugii_payload_version_verbatim() {
    let matched = NxDialect::classify(&container(true, 0x0a));

    assert_eq!(
        matched.dialect.as_ref().map(DialectId::as_str),
        Some("nx:legacy-cfb")
    );
    assert_eq!(matched.declared[DECLARED_UGII_VERSION], "10");
    assert!(!matched.declared.contains_key(DECLARED_SPLMSSTR_VERSION));
    assert!(!matched.declared.contains_key(DECLARED_PRODUCT_VERSION));
}

#[test]
fn the_version_byte_is_evidence_and_never_moves_the_resolved_id() {
    // The byte is read with `unwrap_or(0)` and compared to nothing, so every
    // value lands on the same row as every other. A consumer that parsed a
    // version out of the id, or expected the id to agree with the declaration
    // beside it, would be reading a field this codec does not branch on.
    for version in [0_u8, 1, 6, 255] {
        let matched = NxDialect::classify(&container(false, version));
        assert_eq!(
            matched.dialect.as_ref().map(DialectId::as_str),
            Some("nx:splmsstr")
        );
        assert_eq!(
            matched.declared[DECLARED_SPLMSSTR_VERSION],
            version.to_string()
        );
        assert_eq!(matched.admission, Admission::Admitted);
    }
}
