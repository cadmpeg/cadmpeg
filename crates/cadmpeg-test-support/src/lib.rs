// SPDX-License-Identifier: Apache-2.0
//! Test-only helpers shared by cadmpeg codec crates.
//!
//! This crate is `publish = false`. Production crates must not depend on it.

use std::collections::BTreeSet;

use cadmpeg_core::dialect::DialectId;

pub mod golden;
pub mod roundtrip;

/// Assert that a codec enum and its generated registry rows are equal.
pub fn assert_dialect_rows_closed(ids: &[DialectId], rows: &[DialectId]) {
    let enum_ids = ids.iter().map(DialectId::as_str).collect::<BTreeSet<_>>();
    let registry_ids = rows.iter().map(DialectId::as_str).collect::<BTreeSet<_>>();

    assert_eq!(
        enum_ids.len(),
        ids.len(),
        "two enum variants pin the same id"
    );
    assert_eq!(
        registry_ids.len(),
        rows.len(),
        "two registry rows pin the same id"
    );
    assert_eq!(
        enum_ids, registry_ids,
        "the codec enum and generated registry rows disagree"
    );
}

/// Dialect ids under one format prefix in the identity registry.
///
/// The registry is embedded so every codec drift test parses the same TOML
/// bytes without locating the workspace from its own manifest directory.
#[must_use]
pub fn registry_ids(prefix: &str) -> BTreeSet<String> {
    let registry: toml::Value = toml::from_str(include_str!("../../../docs/dialects.toml"))
        .expect("docs/dialects.toml parses as TOML");
    let prefix = format!("{prefix}:");
    let ids = registry
        .get("dialect")
        .and_then(toml::Value::as_array)
        .expect("docs/dialects.toml declares dialect rows")
        .iter()
        .filter_map(|row| row.get("id"))
        .filter_map(toml::Value::as_str)
        .filter(|id| id.starts_with(&prefix))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no {prefix} rows");
    ids
}
