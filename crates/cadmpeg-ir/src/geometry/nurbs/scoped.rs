// SPDX-License-Identifier: Apache-2.0
//! Immutable NURBS scratch rows.

use std::marker::PhantomData;
use std::ops::Deref;

/// Temporary rows read through a slice.
#[derive(Debug)]
pub struct ScopedRows<'ctx, T> {
    rows: Vec<T>,
    _ctx: PhantomData<&'ctx ()>,
}

impl<T> ScopedRows<'_, T> {
    pub(crate) fn new(rows: Vec<T>) -> Self {
        Self {
            rows,
            _ctx: PhantomData,
        }
    }
}

impl<T> Deref for ScopedRows<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}
