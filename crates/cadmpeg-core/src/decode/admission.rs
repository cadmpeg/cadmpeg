// SPDX-License-Identifier: Apache-2.0
//! Typed admission for code shared by decode and non-decode paths.
//!
//! Decode passes its [`DecodeContext`], whose implementation forwards each
//! method to the one core operation of the same name, so the method charges
//! the session budget exactly as that operation does. Writers and other
//! non-decode callers pass [`StandardAdmission`], which performs the same
//! operation with standard allocation and never refuses: its error type is
//! [`Infallible`].
//!
//! The traits are sealed: these are their only implementations. The decode
//! policy checker proves a call through [`Admission`] or [`AdmissionScope`]
//! as the core operation the `DecodeContext` or `ScopedReservation`
//! implementation forwards to, instantiated with the call's own arguments,
//! and reports a non-decode admission in decode code. An extension trait in
//! another crate takes [`Admission`] as a supertrait, so it too has only
//! these two implementors, and its `DecodeContext` implementation forwards
//! each method to one operation in the same way.

use std::borrow::Borrow;
use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::fmt;
use std::hash::{BuildHasher, Hash};

use super::cost::DecodeCost;
use super::{DecodeContext, DepthGuard, ResourceLimit, ScopedReservation};
use crate::CodecError;

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::DecodeContext<'_> {}
    impl Sealed for super::StandardAdmission {}
    impl Sealed for super::ScopedReservation<'_> {}
    impl Sealed for super::StandardScope {}
}

/// Temporary storage owned by an [`Admission`] scope.
pub trait AdmissionScope: sealed::Sealed {
    /// Refusal produced by the admission policy.
    type Error;

    /// Runs `build`, holding the temporary storage it allocates in this scope.
    fn with_storage<T, E: From<Self::Error>>(
        &mut self,
        build: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E>;
}

/// Work and storage admission for code shared by decode and writers.
///
/// Each method has the name, operands and behaviour of the `DecodeContext`
/// operation it forwards to; its documentation is there.
pub trait Admission: sealed::Sealed {
    /// Refusal produced by the admission policy.
    type Error;
    /// Owner of temporary storage, released when dropped.
    type Scope<'scope>: AdmissionScope<Error = Self::Error>
    where
        Self: 'scope;
    /// Owner of one recursive nesting level, released when dropped.
    type Depth<'scope>
    where
        Self: 'scope;

    /// See [`DecodeContext::charge_work`].
    fn charge_work(&self, units: u64, operation: &'static str) -> Result<(), Self::Error>;

    /// See [`DecodeContext::enter_nested`].
    fn enter_nested(&self, operation: &'static str) -> Result<Self::Depth<'_>, Self::Error>;

    /// See [`DecodeContext::resource_refusal`].
    fn resource_refusal(&self) -> Option<ResourceLimit>;

    /// See [`DecodeContext::reserve_scoped`].
    fn reserve_scoped(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<Self::Scope<'_>, Self::Error>;

    /// See [`DecodeContext::format_retained`].
    fn format_retained(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, Self::Error>;

    /// See [`DecodeContext::equal_bytes`].
    fn equal_bytes(
        &self,
        left: &[u8],
        right: &[u8],
        operation: &'static str,
    ) -> Result<bool, Self::Error>;

    /// See [`DecodeContext::is_sorted_by`].
    fn is_sorted_by<T, K: DecodeCost + ?Sized>(
        &self,
        values: &[T],
        key: impl Fn(&T) -> &K,
        compare: impl FnMut(&K, &K) -> std::cmp::Ordering,
        operation: &'static str,
    ) -> Result<bool, Self::Error>;

    /// See [`DecodeContext::stable_sort_by`].
    fn stable_sort_by<T, K: DecodeCost + ?Sized>(
        &self,
        values: &mut [T],
        key: impl Fn(&T) -> &K,
        compare: impl FnMut(&K, &K) -> std::cmp::Ordering,
        operation: &'static str,
    ) -> Result<(), Self::Error>;

    /// See [`DecodeContext::get_hash_map`].
    fn get_hash_map<'values, K, Q, V, S>(
        &self,
        values: &'values HashMap<K, V, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&'values V>, Self::Error>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher;

    /// See [`DecodeContext::retain_btree_map`].
    fn retain_btree_map<K: Ord, V, E: From<Self::Error>>(
        &self,
        values: &mut BTreeMap<K, V>,
        keep: impl FnMut(&K, &mut V) -> Result<bool, E>,
        operation: &'static str,
    ) -> Result<(), E>;
}

impl AdmissionScope for ScopedReservation<'_> {
    type Error = CodecError;

    #[inline]
    fn with_storage<T, E: From<CodecError>>(
        &mut self,
        build: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        ScopedReservation::with_storage(self, build)
    }
}

impl Admission for DecodeContext<'_> {
    type Error = CodecError;
    type Scope<'scope>
        = ScopedReservation<'scope>
    where
        Self: 'scope;
    type Depth<'scope>
        = DepthGuard<'scope>
    where
        Self: 'scope;

    #[inline]
    fn charge_work(&self, units: u64, operation: &'static str) -> Result<(), CodecError> {
        DecodeContext::charge_work(self, units, operation)
    }

    #[inline]
    fn enter_nested(&self, operation: &'static str) -> Result<DepthGuard<'_>, CodecError> {
        DecodeContext::enter_nested(self, operation)
    }

    #[inline]
    fn resource_refusal(&self) -> Option<ResourceLimit> {
        DecodeContext::resource_refusal(self)
    }

    #[inline]
    fn reserve_scoped(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, CodecError> {
        DecodeContext::reserve_scoped(self, bytes, operation)
    }

    #[inline]
    fn format_retained(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        DecodeContext::format_retained(self, args, operation)
    }

    #[inline]
    fn equal_bytes(
        &self,
        left: &[u8],
        right: &[u8],
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        DecodeContext::equal_bytes(self, left, right, operation)
    }

    #[inline]
    fn is_sorted_by<T, K: DecodeCost + ?Sized>(
        &self,
        values: &[T],
        key: impl Fn(&T) -> &K,
        compare: impl FnMut(&K, &K) -> std::cmp::Ordering,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        DecodeContext::is_sorted_by(self, values, key, compare, operation)
    }

    #[inline]
    fn stable_sort_by<T, K: DecodeCost + ?Sized>(
        &self,
        values: &mut [T],
        key: impl Fn(&T) -> &K,
        compare: impl FnMut(&K, &K) -> std::cmp::Ordering,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        DecodeContext::stable_sort_by(self, values, key, compare, operation)
    }

    #[inline]
    fn get_hash_map<'values, K, Q, V, S>(
        &self,
        values: &'values HashMap<K, V, S>,
        key: &Q,
        operation: &'static str,
    ) -> Result<Option<&'values V>, CodecError>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        DecodeContext::get_hash_map(self, values, key, operation)
    }

    #[inline]
    fn retain_btree_map<K: Ord, V, E: From<CodecError>>(
        &self,
        values: &mut BTreeMap<K, V>,
        keep: impl FnMut(&K, &mut V) -> Result<bool, E>,
        operation: &'static str,
    ) -> Result<(), E> {
        DecodeContext::retain_btree_map(self, values, keep, operation)
    }
}

/// Admission for code that runs outside decode: standard allocation, no
/// budget, and no refusal.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandardAdmission;

/// The temporary storage scope of [`StandardAdmission`], which holds nothing.
#[derive(Debug, Default)]
pub struct StandardScope;

impl AdmissionScope for StandardScope {
    type Error = Infallible;

    fn with_storage<T, E: From<Infallible>>(
        &mut self,
        build: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        build()
    }
}

impl Admission for StandardAdmission {
    type Error = Infallible;
    type Scope<'scope> = StandardScope;
    type Depth<'scope> = ();

    fn charge_work(&self, _units: u64, _operation: &'static str) -> Result<(), Infallible> {
        Ok(())
    }

    fn enter_nested(&self, _operation: &'static str) -> Result<(), Infallible> {
        Ok(())
    }

    fn resource_refusal(&self) -> Option<ResourceLimit> {
        None
    }

    fn reserve_scoped(
        &self,
        _bytes: u64,
        _operation: &'static str,
    ) -> Result<StandardScope, Infallible> {
        Ok(StandardScope)
    }

    fn format_retained(
        &self,
        args: fmt::Arguments<'_>,
        _operation: &'static str,
    ) -> Result<String, Infallible> {
        Ok(fmt::format(args))
    }

    fn equal_bytes(
        &self,
        left: &[u8],
        right: &[u8],
        _operation: &'static str,
    ) -> Result<bool, Infallible> {
        Ok(left == right)
    }

    fn is_sorted_by<T, K: DecodeCost + ?Sized>(
        &self,
        values: &[T],
        key: impl Fn(&T) -> &K,
        mut compare: impl FnMut(&K, &K) -> std::cmp::Ordering,
        _operation: &'static str,
    ) -> Result<bool, Infallible> {
        for pair in values.windows(2) {
            if compare(key(&pair[1]), key(&pair[0])).is_lt() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn stable_sort_by<T, K: DecodeCost + ?Sized>(
        &self,
        values: &mut [T],
        key: impl Fn(&T) -> &K,
        mut compare: impl FnMut(&K, &K) -> std::cmp::Ordering,
        _operation: &'static str,
    ) -> Result<(), Infallible> {
        values.sort_by(|left, right| compare(key(left), key(right)));
        Ok(())
    }

    fn get_hash_map<'values, K, Q, V, S>(
        &self,
        values: &'values HashMap<K, V, S>,
        key: &Q,
        _operation: &'static str,
    ) -> Result<Option<&'values V>, Infallible>
    where
        K: Borrow<Q> + Eq + Hash,
        Q: DecodeCost + Eq + Hash + ?Sized,
        S: BuildHasher,
    {
        Ok(values.get(key))
    }

    fn retain_btree_map<K: Ord, V, E: From<Infallible>>(
        &self,
        values: &mut BTreeMap<K, V>,
        mut keep: impl FnMut(&K, &mut V) -> Result<bool, E>,
        _operation: &'static str,
    ) -> Result<(), E> {
        let mut refusal = None;
        values.retain(|key, value| {
            if refusal.is_some() {
                return true;
            }
            keep(key, value).unwrap_or_else(|error| {
                refusal = Some(error);
                true
            })
        });
        refusal.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::{Admission, StandardAdmission};

    #[test]
    fn standard_admission_uses_infallible_nesting_and_stable_sorting() {
        let admission = StandardAdmission;
        assert_eq!(admission.enter_nested("nested"), Ok(()));
        assert_eq!(admission.resource_refusal(), None);

        let mut values = [(2_u8, 'a'), (1, 'b'), (2, 'c'), (1, 'd')];
        assert!(!admission
            .is_sorted_by(&values, |value| &value.0, Ord::cmp, "sorted")
            .expect("standard comparison cannot refuse"));
        admission
            .stable_sort_by(&mut values, |value| &value.0, Ord::cmp, "sort")
            .expect("standard sort cannot refuse");
        assert_eq!(values, [(1, 'b'), (1, 'd'), (2, 'a'), (2, 'c')]);
        assert!(admission
            .is_sorted_by(&values, |value| &value.0, Ord::cmp, "sorted")
            .expect("standard comparison cannot refuse"));
    }
}
