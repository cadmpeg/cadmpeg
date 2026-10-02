// SPDX-License-Identifier: Apache-2.0
//! Shared cage validation with explicit reconstruction storage.

use std::collections::BTreeSet;
use std::fmt::Arguments;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;

use super::SubdError;

pub(super) trait SubdAdmission {
    type Error;
    type Storage<'ctx> where Self: 'ctx;

    fn work(&self, count: u64, operation: &'static str) -> Result<(), Self::Error>;
    fn message(&self, message: Arguments<'_>) -> Result<SubdError, Self::Error>;
    fn storage(&self) -> Result<Self::Storage<'_>, Self::Error>;
    fn insert<T: Ord>(&self, storage: &mut Self::Storage<'_>, values: &mut BTreeSet<T>, value: T) -> Result<bool, Self::Error>;
}

pub(super) struct StandardAdmission;

impl SubdAdmission for StandardAdmission {
    type Error = SubdError;
    type Storage<'ctx> = ();

    fn work(&self, _count: u64, _operation: &'static str) -> Result<(), SubdError> { Ok(()) }
    fn message(&self, message: Arguments<'_>) -> Result<SubdError, SubdError> { Ok(SubdError::Admission(message.to_string())) }
    fn storage(&self) -> Result<(), SubdError> { Ok(()) }
    fn insert<T: Ord>(&self, _storage: &mut (), values: &mut BTreeSet<T>, value: T) -> Result<bool, SubdError> { Ok(values.insert(value)) }
}

impl SubdAdmission for DecodeContext<'_> {
    type Error = CodecError;
    type Storage<'ctx> = ScopedReservation<'ctx> where Self: 'ctx;

    fn work(&self, count: u64, operation: &'static str) -> Result<(), CodecError> { self.charge_work(count, operation) }
    fn message(&self, message: Arguments<'_>) -> Result<SubdError, CodecError> { Ok(SubdError::Admission(self.format_retained(message, "SubD admission error")?)) }
    fn storage(&self) -> Result<ScopedReservation<'_>, CodecError> { self.reserve_scoped(0, "SubD validation members") }
    fn insert<T: Ord>(&self, storage: &mut ScopedReservation<'_>, values: &mut BTreeSet<T>, value: T) -> Result<bool, CodecError> {
        self.insert_scoped_btree_set(storage, values, value, "SubD validation member search", "SubD validation members")
    }
}
