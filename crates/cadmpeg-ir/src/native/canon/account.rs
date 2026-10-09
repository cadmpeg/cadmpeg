// SPDX-License-Identifier: Apache-2.0
//! The canonical serializer's decode or Standard operation route.

use std::convert::Infallible;
use std::fmt;

use cadmpeg_core::decode::admission::{Admission, StandardAdmission};
use cadmpeg_core::decode::{DecodeContext, DepthGuard, ScopedReservation};
use cadmpeg_core::CodecError;

#[derive(Clone, Copy)]
pub(in crate::native) enum Account<'a> {
    Decode(&'a DecodeContext<'a>),
    Standard,
}

fn standard<T>(result: Result<T, Infallible>) -> Result<T, CodecError> {
    match result {
        Ok(value) => Ok(value),
        Err(never) => match never {},
    }
}

impl<'a> Account<'a> {
    pub(in crate::native) fn charge_work(self, work: u64, operation: &'static str) -> Result<(), CodecError> {
        match self {
            Self::Decode(ctx) => ctx.charge_work(work, operation),
            Self::Standard => standard(StandardAdmission.charge_work(work, operation)),
        }
    }

    pub(super) fn enter_nested(self, operation: &'static str) -> Result<Option<DepthGuard<'a>>, CodecError> {
        match self {
            Self::Decode(ctx) => ctx.enter_nested(operation).map(Some),
            Self::Standard => standard(StandardAdmission.enter_nested(operation)).map(|()| None),
        }
    }

    pub(super) fn format_retained(self, args: fmt::Arguments<'_>, operation: &'static str) -> Result<String, CodecError> {
        match self {
            Self::Decode(ctx) => ctx.format_retained(args, operation),
            Self::Standard => standard(StandardAdmission.format_retained(args, operation)),
        }
    }

    pub(super) fn try_reserve_retained_text(self, text: &mut String, additional: usize, operation: &'static str) -> Result<(), CodecError> {
        match self {
            Self::Decode(ctx) => ctx.try_reserve_retained_text(text, additional, operation),
            Self::Standard => {
                text.reserve_exact(additional);
                Ok(())
            }
        }
    }

    pub(super) fn reserve_vec<T>(self, values: &mut Vec<T>, additional: usize, operation: &'static str) -> Result<(), CodecError> {
        match self {
            Self::Decode(ctx) => ctx.reserve_vec(values, additional, operation),
            Self::Standard => {
                values.reserve(additional);
                Ok(())
            }
        }
    }

    pub(super) fn collection_vec<T>(self, count: usize, operation: &'static str) -> Result<Vec<T>, CodecError> {
        match self {
            Self::Decode(ctx) => ctx.collection_vec(count, operation),
            Self::Standard => Ok(Vec::with_capacity(count)),
        }
    }

    pub(super) fn admit_btree_node_storage<K, V>(self, len: usize, operation: &'static str) -> Result<(), CodecError> {
        match self {
            Self::Decode(ctx) => ctx.admit_btree_node_storage::<K, V>(len, operation),
            Self::Standard => Ok(()),
        }
    }

    pub(super) fn charge_collection_items(self, count: u64, operation: &'static str) -> Result<(), CodecError> {
        match self {
            Self::Decode(ctx) => ctx.charge_collection_items(count, operation),
            Self::Standard => Ok(()),
        }
    }

    pub(super) fn refuse_codec_limit(self, operation: &'static str, limit: u64, additional: u64) -> CodecError {
        match self {
            Self::Decode(ctx) => ctx.refuse_codec_limit(operation, limit, additional),
            Self::Standard => CodecError::malformed("canonical native storage size overflows"),
        }
    }

    pub(super) fn with_scoped_storage<T, E: From<CodecError>>(self, operation: &'static str, build: impl FnOnce() -> Result<T, E>) -> Result<(T, Option<ScopedReservation<'a>>), E> {
        match self {
            Self::Decode(ctx) => ctx.with_scoped_storage(operation, build).map(|(value, storage)| (value, Some(storage))),
            Self::Standard => build().map(|value| (value, None)),
        }
    }
}
