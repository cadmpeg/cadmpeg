// SPDX-License-Identifier: Apache-2.0
//! Labelled reads over a [`View`], shared by this crate's record cursors.
//!
//! Each read attaches the caller's field name to a truncation, so the
//! diagnostic states the field, the space and the offset. A cursor that
//! reports a truncation as `CodecError::Malformed`, and therefore carries no
//! source location, keeps its own reader.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

pub(crate) fn u8(view: &mut View<'_>, field: &'static str) -> Result<u8, CodecError> {
    Ok(view.req_u8().map_err(|error| error.during(field))?)
}

pub(crate) fn u16(view: &mut View<'_>, field: &'static str) -> Result<u16, CodecError> {
    Ok(view.req_u16_le().map_err(|error| error.during(field))?)
}

pub(crate) fn i16(view: &mut View<'_>, field: &'static str) -> Result<i16, CodecError> {
    Ok(view.req_i16_le().map_err(|error| error.during(field))?)
}

pub(crate) fn u32(view: &mut View<'_>, field: &'static str) -> Result<u32, CodecError> {
    Ok(view.req_u32_le().map_err(|error| error.during(field))?)
}

pub(crate) fn i32(view: &mut View<'_>, field: &'static str) -> Result<i32, CodecError> {
    Ok(view.req_i32_le().map_err(|error| error.during(field))?)
}

pub(crate) fn u64(view: &mut View<'_>, field: &'static str) -> Result<u64, CodecError> {
    Ok(view.req_u64_le().map_err(|error| error.during(field))?)
}

pub(crate) fn take<'a>(
    view: &mut View<'a>,
    len: usize,
    field: &'static str,
) -> Result<&'a [u8], CodecError> {
    Ok(view.req_take(len).map_err(|error| error.during(field))?)
}

/// The read position inside `view`, counted from the start of `view`.
pub(crate) fn position(view: &View<'_>) -> usize {
    view.position().saturating_sub(view.start())
}
