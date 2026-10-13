// SPDX-License-Identifier: Apache-2.0
//! Rows keyed by a native identifier, with the identifiers that occur once
//! indexed when the rows are scanned.
//!
//! Decode asks "the row with this identifier, when exactly one exists" for
//! every surface it transfers. Answering by scanning the rows makes each
//! transfer pass quadratic in the row count; the index answers it with one
//! hash lookup of a `u32` key, which is constant work.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// A row that carries its native identifier.
pub(crate) trait NativeRowId {
    /// The identifier the uniqueness index keys on.
    fn native_row_id(&self) -> u32;
}

impl NativeRowId for super::SurfaceRow {
    fn native_row_id(&self) -> u32 {
        self.id
    }
}

impl NativeRowId for super::SurfaceParameterRecord {
    fn native_row_id(&self) -> u32 {
        self.surface_id
    }
}

/// Rows in stored order and the position of each identifier that occurs in
/// exactly one of them. A repeated identifier keeps a `None` marker.
#[derive(Debug, Clone)]
pub(crate) struct UniqueIdRows<T> {
    rows: Vec<T>,
    unique: HashMap<u32, Option<usize>>,
}

impl<T> Default for UniqueIdRows<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            unique: HashMap::new(),
        }
    }
}

impl<T> std::ops::Deref for UniqueIdRows<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.rows
    }
}

impl<T: NativeRowId> UniqueIdRows<T> {
    /// Indexes `rows`, admitting one hash entry per distinct identifier.
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        rows: Vec<T>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let mut unique = HashMap::new();
        for (position, row) in ctx.admit_iter(&rows, operation)?.enumerate() {
            match ctx.entry_hash_map(&mut unique, row.native_row_id(), operation)? {
                Entry::Vacant(entry) => {
                    entry.insert(Some(position));
                }
                Entry::Occupied(mut entry) => {
                    *entry.get_mut() = None;
                }
            }
        }
        Ok(Self { rows, unique })
    }

    /// Whether any row carries `id`.
    pub(crate) fn contains_id(&self, id: u32) -> bool {
        self.unique.contains_key(&id)
    }

    /// The row with `id` when exactly one row carries it.
    pub(crate) fn unique(&self, id: u32) -> Option<&T> {
        let position = (*self.unique.get(&id)?)?;
        self.rows.get(position)
    }
}

#[cfg(test)]
impl<T: NativeRowId> UniqueIdRows<T> {
    /// Indexes fixture rows outside a decode session.
    pub(crate) fn from_rows(rows: Vec<T>) -> Self {
        let mut unique = HashMap::new();
        for (position, row) in rows.iter().enumerate() {
            unique
                .entry(row.native_row_id())
                .and_modify(|slot| *slot = None)
                .or_insert(Some(position));
        }
        Self { rows, unique }
    }

    /// Edits fixture rows and indexes them again.
    pub(crate) fn edit<R>(&mut self, edit: impl FnOnce(&mut Vec<T>) -> R) -> R {
        let result = edit(&mut self.rows);
        *self = Self::from_rows(std::mem::take(&mut self.rows));
        result
    }

    /// Appends a fixture row.
    pub(crate) fn push(&mut self, row: T) {
        self.edit(|rows| rows.push(row));
    }

    /// Appends fixture rows.
    pub(crate) fn extend(&mut self, rows: impl IntoIterator<Item = T>) {
        self.edit(|stored| stored.extend(rows));
    }

    /// Removes the last fixture row.
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.edit(Vec::pop)
    }

    /// Removes every fixture row.
    pub(crate) fn clear(&mut self) {
        self.edit(Vec::clear);
    }

    /// Fixture rows for edits that keep every identifier; an identifier
    /// change goes through [`Self::edit`] so the index follows it.
    pub(crate) fn fields_mut(&mut self) -> &mut [T] {
        &mut self.rows
    }

    /// Keeps the first `len` fixture rows.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.edit(|rows| rows.truncate(len));
    }
}

#[cfg(test)]
impl<T: NativeRowId> FromIterator<T> for UniqueIdRows<T> {
    fn from_iter<I: IntoIterator<Item = T>>(rows: I) -> Self {
        Self::from_rows(rows.into_iter().collect())
    }
}

#[cfg(test)]
impl<T: NativeRowId> From<Vec<T>> for UniqueIdRows<T> {
    fn from(rows: Vec<T>) -> Self {
        Self::from_rows(rows)
    }
}
