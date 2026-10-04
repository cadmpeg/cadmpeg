// SPDX-License-Identifier: Apache-2.0
//! One membership algorithm with decode and standard allocation policies.

use std::cell::RefCell;
use std::collections::{HashSet, TryReserveError};
use std::hash::{Hash, Hasher};

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) trait Admission<T: PartialEq>: Sized {
    type Error;
    fn index(&self, count: usize) -> Result<Index<'_, T, Self>, Self::Error>
    where
        T: Eq + Hash;
    fn compare(&self, left: &T, right: &T) -> Result<bool, Self::Error>;
    fn compare_key(&self, left: &T, right: &T) -> bool;
    fn finish_key_operation(&self) -> Result<(), Self::Error>;
    fn work(&self, count: usize) -> Result<(), Self::Error>;
}

pub(super) struct StandardAdmission;

impl<T: PartialEq> Admission<T> for StandardAdmission {
    type Error = TryReserveError;
    fn index(&self, count: usize) -> Result<Index<'_, T, Self>, Self::Error>
    where
        T: Eq + Hash,
    {
        let mut values = HashSet::new();
        values.try_reserve(count)?;
        Ok(Index {
            values,
            admission: self,
            _storage: None,
        })
    }
    fn compare(&self, left: &T, right: &T) -> Result<bool, Self::Error> {
        <Self as Admission<T>>::work(self, 1)?;
        Ok(left == right)
    }
    fn compare_key(&self, left: &T, right: &T) -> bool {
        left == right
    }
    fn finish_key_operation(&self) -> Result<(), Self::Error> {
        <Self as Admission<T>>::work(self, 0)
    }
    fn work(&self, _count: usize) -> Result<(), Self::Error> {
        Ok(())
    }
}

pub(super) struct DecodeAdmission<'ctx, 'arena> {
    pub(super) ctx: &'ctx DecodeContext<'arena>,
    pub(super) operation: &'static str,
    comparison_error: RefCell<Option<CodecError>>,
}

impl<'ctx, 'arena> DecodeAdmission<'ctx, 'arena> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'arena>, operation: &'static str) -> Self {
        Self {
            ctx,
            operation,
            comparison_error: RefCell::new(None),
        }
    }
}

impl<T: DecodeCost + PartialEq> Admission<T> for DecodeAdmission<'_, '_> {
    type Error = CodecError;
    fn index(&self, count: usize) -> Result<Index<'_, T, Self>, Self::Error>
    where
        T: Eq + Hash,
    {
        let (values, storage) = self.ctx.temporary_set_limit(count, self.operation)?;
        Ok(Index {
            values,
            admission: self,
            _storage: Some(storage),
        })
    }
    fn compare(&self, left: &T, right: &T) -> Result<bool, Self::Error> {
        <Self as Admission<T>>::work(self, 1)?;
        self.ctx.equal(left, right, self.operation)
    }
    fn compare_key(&self, left: &T, right: &T) -> bool {
        if self.comparison_error.borrow().is_some() {
            return true;
        }
        match <Self as Admission<T>>::compare(self, left, right) {
            Ok(equal) => equal,
            Err(error) => {
                let mut pending = self.comparison_error.borrow_mut();
                if pending.is_none() {
                    *pending = Some(error);
                }
                // Stop HashSet's collision walk. Index::insert returns the
                // latched error before it can expose this sentinel result.
                true
            }
        }
    }
    fn finish_key_operation(&self) -> Result<(), Self::Error> {
        if let Some(error) = self.comparison_error.borrow_mut().take() {
            return Err(error);
        }
        <Self as Admission<T>>::work(self, 0)
    }
    fn work(&self, count: usize) -> Result<(), Self::Error> {
        self.ctx
            .charge_work_limit(u64_from_index(count), self.operation)
            .map_err(CodecError::from)
    }
}

/// Borrowed keys drop before their uniqueness-index reservation.
pub(super) struct Index<'scope, T: PartialEq, S: Admission<T>> {
    values: HashSet<MemberKey<'scope, T, S>>,
    admission: &'scope S,
    _storage: Option<ScopedReservation<'scope>>,
}

impl<'scope, T: Eq + Hash, S: Admission<T>> Index<'scope, T, S> {
    pub(super) fn insert(&mut self, value: &'scope T) -> Result<bool, S::Error> {
        self.admission.work(0)?;
        let inserted = self.values.insert(MemberKey {
            value,
            admission: self.admission,
        });
        // The Eq callback cannot return an error. The policy latches it and
        // drains it here before any insertion result can escape.
        self.admission.finish_key_operation()?;
        Ok(inserted)
    }
}

struct MemberKey<'scope, T, S> {
    value: &'scope T,
    admission: &'scope S,
}

impl<T: PartialEq, S: Admission<T>> PartialEq for MemberKey<'_, T, S> {
    fn eq(&self, other: &Self) -> bool {
        self.admission.compare_key(self.value, other.value)
    }
}
impl<T: Eq, S: Admission<T>> Eq for MemberKey<'_, T, S> {}

impl<T: PartialEq + Hash, S: Admission<T>> Hash for MemberKey<'_, T, S> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if self.admission.work(1).is_err() {
            return;
        }
        self.value.hash(&mut MemberHasher {
            state,
            admission: self.admission,
            _value: std::marker::PhantomData,
        });
    }
}

struct MemberHasher<'scope, H, S, T> {
    state: &'scope mut H,
    admission: &'scope S,
    _value: std::marker::PhantomData<fn(&T)>,
}

impl<H: Hasher, S: Admission<T>, T: PartialEq> Hasher for MemberHasher<'_, H, S, T> {
    fn finish(&self) -> u64 {
        if self.admission.work(1).is_err() {
            return 0;
        }
        self.state.finish()
    }
    fn write(&mut self, bytes: &[u8]) {
        if self.admission.work(bytes.len()).is_err() {
            return;
        }
        self.state.write(bytes);
    }
}

pub(super) fn distinct<T: Eq + Hash, S: Admission<T>>(
    admission: &S,
    values: &[T],
    count: usize,
    include: impl Fn(&T) -> bool,
) -> Result<bool, S::Error> {
    let mut index = admission.index(count)?;
    for value in values {
        admission.work(1)?;
        if include(value) && !index.insert(value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Admission for ordered insertion, including context-free iterator reconstruction.
pub(super) trait AppendAdmission<T: PartialEq> {
    type Error;
    fn work(&self, count: usize) -> Result<(), Self::Error>;
    fn push(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error>;
    fn compare(&self, left: &T, right: &T) -> Result<bool, Self::Error>;
}

impl<T: PartialEq> AppendAdmission<T> for StandardAdmission {
    type Error = std::convert::Infallible;
    fn work(&self, _count: usize) -> Result<(), Self::Error> {
        Ok(())
    }
    fn push(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error> {
        values.push(value);
        Ok(())
    }
    fn compare(&self, left: &T, right: &T) -> Result<bool, Self::Error> {
        <Self as AppendAdmission<T>>::work(self, 1)?;
        Ok(left == right)
    }
}

impl<T: DecodeCost + PartialEq> AppendAdmission<T> for DecodeAdmission<'_, '_> {
    type Error = CodecError;
    fn work(&self, count: usize) -> Result<(), Self::Error> {
        self.ctx
            .charge_work_limit(u64_from_index(count), self.operation)
            .map_err(CodecError::from)
    }
    fn push(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error> {
        self.ctx
            .reserve_vec_limit(values, 1, self.operation)
            .map_err(CodecError::from)?;
        values.push(value);
        Ok(())
    }
    fn compare(&self, left: &T, right: &T) -> Result<bool, Self::Error> {
        <Self as AppendAdmission<T>>::work(self, 1)?;
        self.ctx.equal(left, right, self.operation)
    }
}

pub(super) fn insert<T: PartialEq, S: AppendAdmission<T>>(
    admission: &S,
    values: &mut Vec<T>,
    value: T,
) -> Result<bool, S::Error> {
    admission.work(0)?;
    for member in values.iter() {
        if admission.compare(member, &value)? {
            return Ok(false);
        }
    }
    admission.push(values, value)?;
    Ok(true)
}

#[cfg(test)]
mod tests;
