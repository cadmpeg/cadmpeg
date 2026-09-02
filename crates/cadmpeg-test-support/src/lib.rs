// SPDX-License-Identifier: Apache-2.0
//! Test-only helpers shared by cadmpeg codec crates.
//!
//! This crate is `publish = false`. Production crates must not depend on it.

use std::collections::BTreeSet;
use std::ops::{Deref, DerefMut};

use cadmpeg_core::dialect::DialectId;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::{CadIr, DecodeReport, SourceFidelity};

pub mod golden;
pub mod roundtrip;

/// Editable parts of a consumed decode result for writer tests.
///
/// This unpublished test crate keeps edit-heavy tests concise without
/// reopening the sealed result type.
#[derive(Debug)]
pub struct EditableDecodeResult {
    ir: CadIr,
    report: DecodeReport,
    source_fidelity: SourceFidelity,
}

impl From<DecodeResult> for EditableDecodeResult {
    fn from(result: DecodeResult) -> Self {
        let (ir, report, source_fidelity) = result.into_parts();
        Self {
            ir,
            report,
            source_fidelity,
        }
    }
}

impl EditableDecodeResult {
    /// Borrow the finalized IR.
    #[must_use]
    pub fn ir(&self) -> &CadIr {
        &self.ir
    }

    /// Edit the IR and restore canonical order when the guard is dropped.
    pub fn ir_mut(&mut self) -> impl DerefMut<Target = CadIr> + '_ {
        FinalizingEdit::new(&mut self.ir, CadIr::finalize)
    }

    /// Borrow the decode report.
    #[must_use]
    pub fn report(&self) -> &DecodeReport {
        &self.report
    }

    /// Borrow source fidelity.
    #[must_use]
    pub fn source_fidelity(&self) -> &SourceFidelity {
        &self.source_fidelity
    }

    /// Edit source fidelity and restore canonical order when the guard drops.
    pub fn source_fidelity_mut(&mut self) -> impl DerefMut<Target = SourceFidelity> + '_ {
        FinalizingEdit::new(&mut self.source_fidelity, SourceFidelity::finalize)
    }

    /// Consume the editable value into IR, report, and source fidelity.
    #[must_use]
    pub fn into_parts(self) -> (CadIr, DecodeReport, SourceFidelity) {
        (self.ir, self.report, self.source_fidelity)
    }
}

#[must_use = "the guard keeps the editable value borrowed until it is finalized"]
struct FinalizingEdit<'a, T> {
    value: &'a mut T,
    finalize: fn(&mut T),
}

impl<'a, T> FinalizingEdit<'a, T> {
    fn new(value: &'a mut T, finalize: fn(&mut T)) -> Self {
        Self { value, finalize }
    }
}

impl<T> Deref for FinalizingEdit<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T> DerefMut for FinalizingEdit<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

impl<T> Drop for FinalizingEdit<'_, T> {
    fn drop(&mut self) {
        (self.finalize)(self.value);
    }
}

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
