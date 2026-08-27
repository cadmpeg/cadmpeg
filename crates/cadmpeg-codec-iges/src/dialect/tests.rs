// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

use super::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Path of the identity registry, from this crate's manifest directory.
fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/dialects.toml")
        .canonicalize()
        .expect("docs/dialects.toml resolves from the crate manifest directory")
}

/// Every `id = "iges:…"` value in `docs/dialects.toml`.
fn registry_ids() -> BTreeSet<String> {
    let text = std::fs::read_to_string(registry_path()).expect("read docs/dialects.toml");
    let ids = text
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("id = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|id| id.starts_with("iges:"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no iges rows");
    ids
}

#[test]
fn every_pinned_id_has_a_registry_row_and_every_row_has_a_variant() {
    let pinned = IgesDialect::ALL
        .iter()
        .map(|dialect| dialect.id().as_str().to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(
        pinned.len(),
        IgesDialect::ALL.len(),
        "two variants pin the same id"
    );
    assert_eq!(
        pinned,
        registry_ids(),
        "docs/dialects.toml and IgesDialect disagree; ids are pinned forever, so reconcile the enum"
    );
}

#[test]
fn the_totality_row_absorbs_the_representation_version_pairs_the_registry_omits() {
    // Fixed ASCII enumerates all eleven clamped flags.
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
