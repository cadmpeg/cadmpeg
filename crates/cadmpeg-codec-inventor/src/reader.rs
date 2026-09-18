// SPDX-License-Identifier: Apache-2.0
//! Labelled reads over a [`View`], shared by this crate's record cursors.
//!
//! Each read attaches the caller's field name to a truncation, so the
//! diagnostic states the field, the space and the offset. Every record cursor
//! in this crate reports a truncation this way, so a consumer parses one form
//! and covers the codec.

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

pub(crate) fn i64(view: &mut View<'_>, field: &'static str) -> Result<i64, CodecError> {
    Ok(view.req_i64_le().map_err(|error| error.during(field))?)
}

pub(crate) fn array<const N: usize>(
    view: &mut View<'_>,
    field: &'static str,
) -> Result<[u8; N], CodecError> {
    let mut values = [0; N];
    for value in &mut values {
        *value = u8(view, field)?;
    }
    Ok(values)
}

pub(crate) fn u16_array<const N: usize>(
    view: &mut View<'_>,
    field: &'static str,
) -> Result<[u16; N], CodecError> {
    let mut values = [0; N];
    for value in &mut values {
        *value = u16(view, field)?;
    }
    Ok(values)
}

pub(crate) fn u32_array<const N: usize>(
    view: &mut View<'_>,
    field: &'static str,
) -> Result<[u32; N], CodecError> {
    let mut values = [0; N];
    for value in &mut values {
        *value = u32(view, field)?;
    }
    Ok(values)
}

pub(crate) fn take<'a>(
    view: &mut View<'a>,
    len: usize,
    field: &'static str,
) -> Result<&'a [u8], CodecError> {
    Ok(view.req_take(len).map_err(|error| error.during(field))?)
}
