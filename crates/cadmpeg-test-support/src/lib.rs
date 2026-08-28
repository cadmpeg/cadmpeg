// SPDX-License-Identifier: Apache-2.0
//! Test-only helpers shared by cadmpeg codec crates.
//!
//! This crate is `publish = false`. Production crates must not depend on it.

use std::collections::BTreeSet;

pub mod golden;
pub mod roundtrip;

use cadmpeg_core::dialect::DialectId;

/// Assert that pinned identity variants and one registry namespace are equal.
pub fn assert_registry_closed(prefix: &str, ids: &[DialectId]) {
    let pinned = ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(pinned.len(), ids.len(), "two variants pin the same id");
    assert_eq!(
        pinned,
        registry_ids(prefix),
        "docs/dialects.toml and the identity enum disagree; ids are pinned forever, so reconcile the enum"
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
