// SPDX-License-Identifier: Apache-2.0
//! Working navigation to genuine primary declarations, never semantic reads.
use std::collections::HashMap;
use std::hash::Hash;

use super::{copy_working, DecodeBudget, RecordBatchId};
use crate::decode::u64_from_index;
use crate::CodecError;

#[derive(Debug)]
pub(super) struct Primary {
    pub(super) id: String,
    pub(super) batch: Option<RecordBatchId>,
    pub(super) active: bool,
    pub(super) retired: bool,
}
#[derive(Debug, Default)]
pub(super) struct PrimaryIndex {
    by_batch: HashMap<Option<RecordBatchId>, HashMap<String, usize>>,
    by_token: HashMap<usize, Primary>,
}
impl PrimaryIndex {
    pub(super) fn get(&self, token: usize) -> Option<&Primary> {
        self.by_token.get(&token)
    }
    pub(super) fn find(&self, batch: Option<RecordBatchId>, id: &str) -> Option<(usize, &Primary)> {
        let token = *self.by_batch.get(&batch)?.get(id)?;
        Some((token, self.by_token.get(&token)?))
    }
    pub(super) fn register(
        &mut self,
        budget: &DecodeBudget,
        batch: Option<RecordBatchId>,
        id: &str,
        token: usize,
    ) -> Result<(), CodecError> {
        if self.find(batch, id).is_some() || self.by_token.contains_key(&token) {
            return Err(CodecError::malformed("duplicate primary record authority"));
        }
        reserve(budget, &mut self.by_token, 1)?;
        let mut fresh = HashMap::new();
        if let Some(rows) = self.by_batch.get_mut(&batch) {
            reserve(budget, rows, 1)?;
        } else {
            reserve(budget, &mut self.by_batch, 1)?;
            reserve(budget, &mut fresh, 1)?;
        }
        let key = copy_working(budget, id)?;
        let primary = Primary {
            id: copy_working(budget, id)?,
            batch,
            active: true,
            retired: false,
        };
        self.by_batch
            .entry(batch)
            .or_insert(fresh)
            .insert(key, token);
        self.by_token.insert(token, primary);
        Ok(())
    }
    pub(super) fn set_active(&mut self, token: usize, active: bool) {
        if let Some(primary) = self.by_token.get_mut(&token) {
            primary.active = active;
        }
    }
    pub(super) fn set_retired(&mut self, token: usize, retired: bool) {
        if let Some(primary) = self.by_token.get_mut(&token) {
            primary.retired = retired;
        }
    }
    pub(super) fn remove(&mut self, token: usize) {
        if let Some(primary) = self.by_token.remove(&token) {
            if let Some(rows) = self.by_batch.get_mut(&primary.batch) {
                rows.remove(&primary.id);
                if rows.is_empty() {
                    self.by_batch.remove(&primary.batch);
                }
            }
        }
    }
    pub(super) fn clear(&mut self) {
        self.by_batch.clear();
        self.by_token.clear();
    }
}
fn reserve<K: Eq + Hash, V>(
    budget: &DecodeBudget,
    values: &mut HashMap<K, V>,
    additional: usize,
) -> Result<(), CodecError> {
    let bytes = additional
        .checked_mul(size_of::<(K, V)>())
        .ok_or_else(|| budget.retained_allocation_failed(u64::MAX, "record authority index"))?;
    values.try_reserve(additional).map_err(|_| {
        budget.retained_allocation_failed(u64_from_index(bytes), "record authority index")
    })
}

#[cfg(test)]
mod tests;
