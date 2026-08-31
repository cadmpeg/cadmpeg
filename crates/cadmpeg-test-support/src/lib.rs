// SPDX-License-Identifier: Apache-2.0
//! Test-only helpers shared by cadmpeg codec crates.
//!
//! This crate is `publish = false`. Production crates must not depend on it.

use std::collections::BTreeSet;

use cadmpeg_core::dialect::DialectId;

pub mod golden;
pub mod roundtrip;

/// Assert that a codec enum and its reportable identity-registry rows are equal.
pub fn assert_dialect_rows_closed(ids: &[DialectId], format: &str) {
    let enum_ids = ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let registry_ids = registry_ids(format);

    assert_eq!(
        enum_ids.len(),
        ids.len(),
        "two enum variants pin the same id"
    );
    assert_eq!(
        enum_ids, registry_ids,
        "the codec enum and identity registry disagree"
    );
}

/// Reportable dialect ids under one format prefix in the identity registry.
///
/// The registry is embedded so every codec drift test parses the same TOML
/// bytes without locating the workspace from its own manifest directory.
/// Rows marked `detect-unreachable` describe identification failures that
/// cannot produce a [`cadmpeg_core::dialect::DialectMatch`], so codec enums do
/// not own variants for them.
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
        .filter(|row| {
            row.get("unknown_kind").and_then(toml::Value::as_str) != Some("detect-unreachable")
        })
        .filter_map(|row| row.get("id").and_then(toml::Value::as_str))
        .filter(|id| id.starts_with(&prefix))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert!(!ids.is_empty(), "the registry declares no {prefix} rows");
    ids
}
