// SPDX-License-Identifier: Apache-2.0
//! Admitted text comparisons and borrowed replacement lookup storage.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::ids::comparison::compare;

/// A cache key whose ordered callbacks charge its owning session.
#[derive(Debug)]
pub(super) struct Key<'ctx> {
    text: Cow<'ctx, str>,
    ctx: &'ctx DecodeContext<'ctx>,
    operation: &'static str,
}

impl<'ctx> Key<'ctx> {
    pub(super) fn owned(ctx: &'ctx DecodeContext<'_>, text: String, operation: &'static str) -> Self {
        Self { text: Cow::Owned(text), ctx, operation }
    }

    pub(super) fn borrowed(ctx: &'ctx DecodeContext<'_>, text: &'ctx str, operation: &'static str) -> Self {
        Self { text: Cow::Borrowed(text), ctx, operation }
    }
}

impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool { self.cmp(other) == Ordering::Equal }
}
impl Eq for Key<'_> {}
impl PartialOrd for Key<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for Key<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match compare(self.ctx, &self.text, &other.text, self.operation) {
            Ok(order) => order,
            // The owner observes the fused session before exposing any map result.
            Err(_) => Ordering::Equal,
        }
    }
}

/// Borrowed replacements retain the source map's ordered iteration.
#[derive(Debug)]
pub(super) struct ReplacementIndex<'ctx> {
    values: Vec<(&'ctx str, &'ctx str)>,
}

impl<'ctx> ReplacementIndex<'ctx> {
    pub(super) fn build(
        ctx: &DecodeContext<'_>, replacements: &'ctx BTreeMap<String, String>,
        storage: &mut ScopedReservation<'_>, operation: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(0, operation)?;
        let mut values = Vec::new();
        ctx.reserve_scoped_vec(storage, &mut values, replacements.len(), operation)?;
        for (source, target) in replacements {
            ctx.charge_work(1, operation)?;
            values.push((source.as_str(), target.as_str()));
        }
        Ok(Self { values })
    }

    pub(super) fn get(
        &self, ctx: &DecodeContext<'_>, source: &str, operation: &'static str,
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
