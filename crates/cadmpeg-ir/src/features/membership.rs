// SPDX-License-Identifier: Apache-2.0
//! One membership algorithm with decode and standard allocation policies.

use std::collections::{HashSet, TryReserveError};
use std::hash::Hash;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};

pub(super) trait Admission<'ctx> {
    type Error;
    fn index<T: Eq + Hash>(&self, count: usize) -> Result<Index<'ctx, T>, Self::Error>;
    fn work(&self, count: usize) -> Result<(), Self::Error>;
    fn member<T: Hash>(&self, value: &T, count: usize, longest: &mut u64) -> Result<(), Self::Error>;
}

pub(super) struct StandardAdmission;

impl<'ctx> Admission<'ctx> for StandardAdmission {
    type Error = TryReserveError;
    fn index<T: Eq + Hash>(&self, count: usize) -> Result<Index<'ctx, T>, Self::Error> {
        let mut values = HashSet::new();
        values.try_reserve(count)?;
        Ok(Index { values, longest: 0, _storage: None })
    }
    fn work(&self, _count: usize) -> Result<(), Self::Error> { Ok(()) }
    fn member<T: Hash>(&self, _value: &T, _count: usize, _longest: &mut u64) -> Result<(), Self::Error> { Ok(()) }
}

pub(super) struct DecodeAdmission<'ctx, 'arena> {
    pub(super) ctx: &'ctx DecodeContext<'arena>,
    pub(super) operation: &'static str,
}

impl<'ctx> Admission<'ctx> for DecodeAdmission<'ctx, '_> {
    type Error = ResourceLimit;
    fn index<T: Eq + Hash>(&self, count: usize) -> Result<Index<'ctx, T>, Self::Error> {
        let (values, storage) = self.ctx.temporary_set_limit(count, self.operation)?;
        Ok(Index { values, longest: 0, _storage: Some(storage) })
    }
    fn work(&self, count: usize) -> Result<(), Self::Error> {
        self.ctx.charge_work_limit(u64_from_index(count), self.operation)
    }
    fn member<T: Hash>(&self, value: &T, count: usize, longest: &mut u64) -> Result<(), Self::Error> {
        super::member_work::admit_member_work(self.ctx, value, count, longest, self.operation)
    }
}

/// Borrowed keys drop before their uniqueness-index reservation.
pub(super) struct Index<'ctx, T> {
    values: HashSet<T>,
    longest: u64,
    _storage: Option<ScopedReservation<'ctx>>,
}

impl<'ctx, T: Eq + Hash> Index<'ctx, T> {
    pub(super) fn insert<S: Admission<'ctx>>(&mut self, admission: &S, value: T) -> Result<bool, S::Error> {
        admission.member(&value, self.values.len(), &mut self.longest)?;
        Ok(self.values.insert(value))
    }
}

pub(super) fn distinct<'ctx, T: Eq + Hash, S: Admission<'ctx>>(
    admission: &S,
    values: &[T],
    count: usize,
    include: impl Fn(&T) -> bool,
) -> Result<bool, S::Error> {
    let mut index = admission.index(count)?;
    for value in values {
        admission.work(1)?;
        if include(value) && !index.insert(admission, value)? { return Ok(false); }
    }
    Ok(true)
}

/// Admission for ordered insertion, including context-free iterator reconstruction.
pub(super) trait AppendAdmission {
    type Error;
    fn work(&self, count: usize) -> Result<(), Self::Error>;
    fn push<T>(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error>;
}

impl AppendAdmission for StandardAdmission {
    type Error = std::convert::Infallible;
    fn work(&self, _count: usize) -> Result<(), Self::Error> { Ok(()) }
    fn push<T>(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error> {
        values.push(value);
        Ok(())
    }
}

impl AppendAdmission for DecodeAdmission<'_, '_> {
    type Error = ResourceLimit;
    fn work(&self, count: usize) -> Result<(), Self::Error> {
        self.ctx.charge_work_limit(u64_from_index(count), self.operation)
    }
    fn push<T>(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error> {
        self.ctx.reserve_retained_vec_limit(values, 1, self.operation)?;
        values.push(value);
        Ok(())
    }
}

pub(super) fn insert<T: PartialEq, S: AppendAdmission>(
    admission: &S,
    values: &mut Vec<T>,
    value: T,
) -> Result<bool, S::Error> {
    admission.work(0)?;
    for member in values.iter() {
        admission.work(1)?;
        if member == &value { return Ok(false); }
    }
    admission.push(values, value)?;
    Ok(true)
}
