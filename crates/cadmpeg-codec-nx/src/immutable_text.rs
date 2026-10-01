// SPDX-License-Identifier: Apache-2.0
//! Immutable borrowed and owned text storage for checked strings.

mod sealed {
    pub(crate) trait Sealed {}
    impl Sealed for String {}
    impl Sealed for &str {}
}

pub(crate) trait ImmutableText: sealed::Sealed + AsRef<str> {}
impl ImmutableText for String {}
impl ImmutableText for &str {}
