// SPDX-License-Identifier: Apache-2.0
//! Typed traversal admission for canonical JSON serialization.

use std::convert::Infallible;
use std::fmt::Display;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, DepthGuard, ResourceLimit};
use cadmpeg_core::CodecError;
use serde::Serializer;

pub(in crate::hash) trait Admission {
    type Error;
    type Depth<'scope>
    where
        Self: 'scope;

    fn enter(&self) -> Result<Self::Depth<'_>, Self::Error>;
    fn text(&self, bytes: usize) -> Result<(), Self::Error>;
    fn resource(error: Self::Error) -> Option<ResourceLimit>;
    fn collect_str<S: Serializer, T: Display + ?Sized>(
        &self,
        inner: S,
        value: &T,
    ) -> Result<Result<S::Ok, S::Error>, Self::Error>;
}

pub(super) struct StandardAdmission;

impl Admission for StandardAdmission {
    type Error = Infallible;
    type Depth<'scope> = ();

    fn enter(&self) -> Result<(), Infallible> {
        Ok(())
    }
    fn text(&self, _bytes: usize) -> Result<(), Infallible> {
        Ok(())
    }
    fn resource(error: Infallible) -> Option<ResourceLimit> {
        match error {}
    }
    fn collect_str<S: Serializer, T: Display + ?Sized>(
        &self,
        inner: S,
        value: &T,
    ) -> Result<Result<S::Ok, S::Error>, Infallible> {
        Ok(inner.collect_str(value))
    }
}

impl Admission for &DecodeContext<'_> {
    type Error = CodecError;
    type Depth<'scope>
        = DepthGuard<'scope>
    where
        Self: 'scope;

    fn enter(&self) -> Result<DepthGuard<'_>, CodecError> {
        self.charge_work(1, "walk document digest")?;
        self.enter_nested("walk document digest")
    }
    fn text(&self, bytes: usize) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(bytes), "scan document digest text")
    }
    fn resource(error: CodecError) -> Option<ResourceLimit> {
        match error {
            CodecError::ResourceLimit(limit) => Some(limit),
            _ => None,
        }
    }
    fn collect_str<S: Serializer, T: Display + ?Sized>(
        &self,
        inner: S,
        value: &T,
    ) -> Result<Result<S::Ok, S::Error>, CodecError> {
        let mut storage = self.reserve_scoped(0, "format document digest text")?;
        let text = storage.with_storage(|| {
            self.format_retained(format_args!("{value}"), "format document digest text")
        })?;
        self.text(text.len())?;
        let result = inner.serialize_str(&text);
        drop(text);
        drop(storage);
        Ok(result)
    }
}
