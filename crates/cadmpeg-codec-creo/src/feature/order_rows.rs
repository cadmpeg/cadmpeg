// SPDX-License-Identifier: Apache-2.0
//! Order-table rows with constant-time identity lookup.

use super::definitions::FeatureOrderRow;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

/// Order rows in stored order, indexed by both identifiers. An identifier
/// held by more than one row keeps a `None` marker, so it resolves to no row.
#[derive(Debug, Clone, Default)]
pub(crate) struct OrderRows {
    rows: Vec<FeatureOrderRow>,
    by_internal: HashMap<u32, Option<usize>>,
    by_external: HashMap<u32, Option<usize>>,
}

impl PartialEq for OrderRows {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
    }
}

impl Eq for OrderRows {}

impl std::ops::Deref for OrderRows {
    type Target = [FeatureOrderRow];

    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

impl<'rows> IntoIterator for &'rows OrderRows {
    type Item = &'rows FeatureOrderRow;
    type IntoIter = std::slice::Iter<'rows, FeatureOrderRow>;

    fn into_iter(self) -> Self::IntoIter {
        self.rows.iter()
    }
}

impl cadmpeg_core::decode::cost::DecodeCost for OrderRows {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(&self.rows, ctx, operation)
    }
}

#[cfg(test)]
fn record(positions: &mut HashMap<u32, Option<usize>>, id: u32, position: usize) {
    match positions.entry(id) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Some(position));
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            *entry.get_mut() = None;
        }
    }
}

impl OrderRows {
    /// Append a row whose identifiers are new to the table. A row repeating
    /// either identifier is not stored and returns `false`.
    pub(crate) fn push_unique(
        &mut self,
        ctx: &DecodeContext<'_>,
        row: FeatureOrderRow,
    ) -> Result<bool, CodecError> {
        if self.by_external.contains_key(&row.external_id)
            || self.by_internal.contains_key(&row.internal_id)
        {
            return Ok(false);
        }
        let position = self.rows.len();
        ctx.reserve_vec(&mut self.rows, 1, "creo order rows")?;
        ctx.entry_hash_map(
            &mut self.by_external,
            row.external_id,
            "creo order external ID index",
        )?
        .or_insert(Some(position));
        ctx.entry_hash_map(
            &mut self.by_internal,
            row.internal_id,
            "creo order internal ID index",
        )?
        .or_insert(Some(position));
        self.rows.push(row);
        Ok(true)
    }

    /// The row holding this generated-entity position, when exactly one does.
    pub(crate) fn by_internal(&self, internal_id: u32) -> Option<&FeatureOrderRow> {
        self.rows.get((*self.by_internal.get(&internal_id)?)?)
    }

    /// The row holding this section entity identifier, when exactly one does.
    pub(crate) fn by_external(&self, external_id: u32) -> Option<&FeatureOrderRow> {
        self.rows.get((*self.by_external.get(&external_id)?)?)
    }

    pub(crate) fn as_slice(&self) -> &[FeatureOrderRow] {
        &self.rows
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Shift every row's source offset by a section base.
    pub(crate) fn add_offset(&mut self, ctx: &DecodeContext<'_>, base: usize) -> Result<(), CodecError> {
        for row in ctx.admit_iter(&mut self.rows, "creo order offset traversal")? {
            row.offset += base;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn push(&mut self, row: FeatureOrderRow) {
        let position = self.rows.len();
        record(&mut self.by_external, row.external_id, position);
        record(&mut self.by_internal, row.internal_id, position);
        self.rows.push(row);
    }
}

#[cfg(test)]
impl From<Vec<FeatureOrderRow>> for OrderRows {
    fn from(rows: Vec<FeatureOrderRow>) -> Self {
        let mut result = Self::default();
        result.extend(rows);
        result
    }
}

#[cfg(test)]
impl FromIterator<FeatureOrderRow> for OrderRows {
    fn from_iter<T: IntoIterator<Item = FeatureOrderRow>>(rows: T) -> Self {
        let mut result = Self::default();
        result.extend(rows);
        result
    }
}

#[cfg(test)]
impl Extend<FeatureOrderRow> for OrderRows {
    fn extend<T: IntoIterator<Item = FeatureOrderRow>>(&mut self, rows: T) {
        for row in rows {
            self.push(row);
        }
    }
}
