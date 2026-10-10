// SPDX-License-Identifier: Apache-2.0
//! One membership algorithm with decode and standard allocation policies.

use std::collections::{HashSet, TryReserveError};
use std::hash::{BuildHasher, Hash, Hasher};

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceLimit, ScopedReservation};
use cadmpeg_core::CodecError;

pub(super) trait Admission: Sized {
    type Error;
    fn index<T: Eq + Hash>(&self, count: usize) -> Result<Index<'_, T, Self>, Self::Error>;
    fn work(&self, count: usize) -> Result<(), Self::Error>;
    fn resource_refusal(&self) -> Option<Self::Error>;
}

pub(super) struct StandardAdmission;

impl Admission for StandardAdmission {
    type Error = TryReserveError;
    fn index<T: Eq + Hash>(&self, count: usize) -> Result<Index<'_, T, Self>, Self::Error> {
        let mut values = HashSet::new();
        values.try_reserve(count)?;
        Ok(Index {
            values,
            admission: self,
            _storage: None,
        })
    }
    fn work(&self, _count: usize) -> Result<(), Self::Error> {
        Ok(())
    }
    fn resource_refusal(&self) -> Option<Self::Error> {
        None
    }
}

pub(super) struct DecodeAdmission<'ctx, 'arena> {
    pub(super) ctx: &'ctx DecodeContext<'arena>,
    pub(super) operation: &'static str,
}

impl Admission for DecodeAdmission<'_, '_> {
    type Error = ResourceLimit;
    fn index<T: Eq + Hash>(&self, count: usize) -> Result<Index<'_, T, Self>, Self::Error> {
        let (values, storage) = self.ctx.temporary_set_limit(count, self.operation)?;
        Ok(Index {
            values,
            admission: self,
            _storage: Some(storage),
        })
    }
    fn work(&self, count: usize) -> Result<(), Self::Error> {
        self.ctx
            .charge_work_limit(u64_from_index(count), self.operation)
    }
    fn resource_refusal(&self) -> Option<Self::Error> {
        self.ctx.resource_refusal()
    }
}

/// Borrowed keys drop before their uniqueness-index reservation.
pub(super) struct Index<'scope, T, S: Admission> {
    values: HashSet<MemberKey<'scope, T, S>>,
    admission: &'scope S,
    _storage: Option<ScopedReservation<'scope>>,
}

impl<T: Eq + Hash, S: Admission> Index<'_, T, S> {
    pub(super) fn insert(&mut self, value: T) -> Result<bool, S::Error> {
        // The member is hashed once here, with the set's own keys, and the
        // set rehashes only the finished hash.
        let mut state = self.values.hasher().build_hasher();
        let mut hasher = MemberHasher {
            state: &mut state,
            admission: self.admission,
            written: 0,
        };
        value.hash(&mut hasher);
        let hashed = hasher.written;
        let hash = hasher.finish();
        if let Some(limit) = self.admission.resource_refusal() {
            return Err(limit);
        }
        let inserted = self.values.insert(MemberKey {
            value,
            admission: self.admission,
            hashed,
            hash,
        });
        // Report a refusal raised by a comparison callback during insertion.
        if let Some(limit) = self.admission.resource_refusal() {
            return Err(limit);
        }
        Ok(inserted)
    }
}

/// A member, the bytes its hash wrote and the finished hash. Comparing two
/// members reads no more than the bytes both of them hashed.
struct MemberKey<'scope, T, S> {
    value: T,
    admission: &'scope S,
    hashed: usize,
    hash: u64,
}

impl<T: PartialEq, S: Admission> PartialEq for MemberKey<'_, T, S> {
    fn eq(&self, other: &Self) -> bool {
        self.admission.work(self.hashed).is_ok()
            && self.admission.work(other.hashed).is_ok()
            && self.value == other.value
    }
}
impl<T: Eq, S: Admission> Eq for MemberKey<'_, T, S> {}

impl<T, S> Hash for MemberKey<'_, T, S> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

/// Hashes a member, admitting each chunk before it is hashed and counting the
/// bytes written.
struct MemberHasher<'scope, H, S> {
    state: &'scope mut H,
    admission: &'scope S,
    written: usize,
}

impl<H: Hasher, S: Admission> Hasher for MemberHasher<'_, H, S> {
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
        // Every written byte was admitted first, so the count stays within
        // the work counter.
        if let Some(written) = self.written.checked_add(bytes.len()) {
            self.written = written;
        }
        self.state.write(bytes);
    }
}

pub(super) fn distinct<T: Eq + Hash, S: Admission>(
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
    fn equal(&self, left: &T, right: &T) -> Result<bool, Self::Error>;
    fn push(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error>;
}

impl<T: PartialEq> AppendAdmission<T> for StandardAdmission {
    type Error = std::convert::Infallible;
    fn work(&self, _count: usize) -> Result<(), Self::Error> {
        Ok(())
    }
    fn equal(&self, left: &T, right: &T) -> Result<bool, Self::Error> {
        Ok(left == right)
    }
    fn push(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error> {
        values.push(value);
        Ok(())
    }
}

impl<T: PartialEq + DecodeCost> AppendAdmission<T> for DecodeAdmission<'_, '_> {
    type Error = CodecError;
    fn work(&self, count: usize) -> Result<(), Self::Error> {
        self.ctx.charge_work(u64_from_index(count), self.operation)
    }
    fn equal(&self, left: &T, right: &T) -> Result<bool, Self::Error> {
        self.ctx.equal(left, right, self.operation)
    }
    fn push(&self, values: &mut Vec<T>, value: T) -> Result<(), Self::Error> {
        self.ctx.push_vec(values, value, self.operation)
    }
}

pub(super) fn insert<T: PartialEq, S: AppendAdmission<T>>(
    admission: &S,
    values: &mut Vec<T>,
    value: T,
) -> Result<bool, S::Error> {
    for member in values.iter() {
        admission.work(1)?;
        if admission.equal(member, &value)? {
            return Ok(false);
        }
    }
    admission.push(values, value)?;
    Ok(true)
}

#[cfg(test)]
mod tests;
