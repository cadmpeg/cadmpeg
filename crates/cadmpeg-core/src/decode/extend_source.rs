// SPDX-License-Identifier: Apache-2.0
//! Closed sources for vector extension without child cloning.

use super::DecodeContext;
use crate::CodecError;

mod sealed {
    pub trait Source<T> {}
}

/// A source whose values extend a vector through charged core operations.
/// Owned values move; borrowed `Copy` values are copied.
pub trait ExtendSource<T>: sealed::Source<T> {
    /// Moves or copies every source value onto the end of `target`.
    fn extend_into(
        self,
        ctx: &DecodeContext<'_>,
        target: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError>;
}

impl<T> sealed::Source<T> for Vec<T> {}
impl<T> ExtendSource<T> for Vec<T> {
    fn extend_into(
        mut self,
        ctx: &DecodeContext<'_>,
        target: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        ctx.append_vec(target, &mut self, operation)
    }
}

impl<T> sealed::Source<T> for Option<T> {}
impl<T> ExtendSource<T> for Option<T> {
    fn extend_into(
        self,
        ctx: &DecodeContext<'_>,
        target: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        match self {
            Some(value) => ctx.push_vec(target, value, operation),
            None => Ok(()),
        }
    }
}

impl<T, const N: usize> sealed::Source<T> for [T; N] {}
impl<T, const N: usize> ExtendSource<T> for [T; N] {
    fn extend_into(
        self,
        ctx: &DecodeContext<'_>,
        target: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        for value in ctx.admit_iter(self, operation)? {
            ctx.push_vec(target, value, operation)?;
        }
        Ok(())
    }
}

impl<T: Copy> sealed::Source<T> for &[T] {}
impl<T: Copy> ExtendSource<T> for &[T] {
    fn extend_into(
        self,
        ctx: &DecodeContext<'_>,
        target: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        ctx.extend_from_slice(target, self, operation)
    }
}

impl<T: Copy> sealed::Source<T> for &Vec<T> {}
impl<T: Copy> ExtendSource<T> for &Vec<T> {
    fn extend_into(
        self,
        ctx: &DecodeContext<'_>,
        target: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        ctx.extend_from_slice(target, self, operation)
    }
}

impl<T: Copy, const N: usize> sealed::Source<T> for &[T; N] {}
impl<T: Copy, const N: usize> ExtendSource<T> for &[T; N] {
    fn extend_into(
        self,
        ctx: &DecodeContext<'_>,
        target: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        ctx.extend_from_slice(target, self, operation)
    }
}
