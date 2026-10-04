// SPDX-License-Identifier: Apache-2.0
//! Vector storage whose reservation follows its values and consuming iterator.

use cadmpeg_core::decode::iter_source::IterSource;
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) struct Scratch<'ctx, T> {
    values: Vec<T>,
    ctx: &'ctx DecodeContext<'ctx>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, T> Scratch<'ctx, T> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            values: Vec::new(),
            ctx,
            storage: ctx.reserve_scoped(0, "validation scratch")?,
        })
    }

    pub(super) fn filter_map<U>(
        ctx: &'ctx DecodeContext<'_>,
        values: impl IntoIterator<Item = U>,
        mut project: impl FnMut(U) -> Result<Option<T>, CodecError>,
    ) -> Result<Self, CodecError> {
        let mut result = Self::new(ctx)?;
        for value in values {
            ctx.charge_work(1, "validation filter scan")?;
            if let Some(value) = project(value)? {
                result.push(value)?;
            }
        }
        Ok(result)
    }

    pub(super) fn pop(&mut self) -> Option<T> {
        self.values.pop()
    }

    pub(super) fn stable_sort_by<K: cadmpeg_core::decode::cost::DecodeCost + ?Sized>(
        &mut self,
        key: impl Fn(&T) -> &K,
        compare: impl FnMut(&K, &K) -> std::cmp::Ordering,
    ) -> Result<(), CodecError> {
        self.ctx.stable_sort_by(
            &mut self.values,
            key,
            compare,
            "sort validation scratch",
        )
    }

    pub(super) fn push(&mut self, value: T) -> Result<(), CodecError> {
        self.ctx.charge_work(1, "validation scratch copy")?;
        self.storage.with_storage(|| {
            self.ctx
                .push_vec(&mut self.values, value, "validation scratch slots")
        })
    }

    pub(super) fn extend<'values, S>(
        &mut self,
        values: &'values S,
        mut project: impl FnMut(<<S as IterSource>::Iter<'values> as Iterator>::Item) -> T,
    ) -> Result<(), CodecError>
    where
        S: IterSource + ?Sized + 'values,
    {
        for value in self.ctx.admit_iter(values, "validation extension scan")? {
            self.push(project(value))?;
        }
        Ok(())
    }
}

impl<T> std::ops::Deref for Scratch<'_, T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

pub(super) struct IntoIter<'ctx, T> {
    values: std::vec::IntoIter<T>,
    _storage: ScopedReservation<'ctx>,
}

impl<T> Iterator for IntoIter<'_, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.values.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl<'ctx, T> IntoIterator for Scratch<'ctx, T> {
    type Item = T;
    type IntoIter = IntoIter<'ctx, T>;
    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            values: self.values.into_iter(),
            _storage: self.storage,
        }
    }
}

#[cfg(test)]
mod tests;
