// SPDX-License-Identifier: Apache-2.0
//! Admitted text comparisons and borrowed replacement lookup storage.

use std::cmp::Ordering;

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::ids::comparison::compare;

/// Borrowed replacements require unique keys in byte order.
#[derive(Debug)]
pub(super) struct ReplacementIndex<'ctx> {
    values: Vec<(&'ctx str, &'ctx str)>,
}

impl<'ctx> ReplacementIndex<'ctx> {
    pub(super) fn build<K: AsRef<str> + 'ctx>(
        ctx: &DecodeContext<'_>,
        replacements: impl ExactSizeIterator<Item = (&'ctx K, &'ctx String)>,
        storage: &mut ScopedReservation<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(0, operation)?;
        let mut values: Vec<(&'ctx str, &'ctx str)> = Vec::new();
        ctx.reserve_scoped_vec(storage, &mut values, replacements.len(), operation)?;
        for (source, target) in replacements {
            ctx.charge_work(1, operation)?;
            let source = source.as_ref();
            if let Some((previous, _)) = values.last() {
                if compare(ctx, previous, source, operation)? != Ordering::Less {
                    return Err(CodecError::malformed(
                        "text replacements must have unique keys in byte order",
                    ));
                }
            }
            values.push((source, target.as_str()));
        }
        Ok(Self { values })
    }

    pub(super) fn get(
        &self,
        ctx: &DecodeContext<'_>,
        source: &str,
        operation: &'static str,
    ) -> Result<Option<&'ctx str>, ResourceLimit> {
        ctx.charge_work_limit(0, operation)?;
        let mut low = 0;
        let mut high = self.values.len();
        while low < high {
            let middle = usize::midpoint(low, high);
            let (candidate, target) = self.values[middle];
            match compare(ctx, candidate, source, operation)? {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => return Ok(Some(target)),
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
