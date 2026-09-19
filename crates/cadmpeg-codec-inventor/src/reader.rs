// SPDX-License-Identifier: Apache-2.0
//! Labelled reads over a [`View`], shared by this crate's record cursors.
//!
//! Each read attaches the caller's field name to a truncation, so the
//! diagnostic states the field, the space and the offset. Every record cursor
//! in this crate reports a truncation this way, so a consumer parses one form
//! and covers the codec.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::pmdc::PmDcReference;

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

/// Reads `N` bytes as one read, so a short window names the array's start.
pub(crate) fn array<const N: usize>(
    view: &mut View<'_>,
    field: &'static str,
) -> Result<[u8; N], CodecError> {
    match view.array::<N>() {
        Some(values) => Ok(values),
        None => Err(CodecError::truncated(view.location(), field)),
    }
}

/// Reads `N` elements, each declared on its own, so a short window names the
/// element the read stopped in.
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

/// Reads `N` elements, each declared on its own, so a short window names the
/// element the read stopped in.
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

/// Reads the packed reference word: the low 31 bits are the index and the top
/// bit states whether the index is qualified.
pub(crate) fn pmdc_reference(
    view: &mut View<'_>,
    field: &'static str,
) -> Result<PmDcReference, CodecError> {
    let value = u32(view, field)?;
    Ok(PmDcReference {
        index: value & 0x7fff_ffff,
        qualified: value & 0x8000_0000 != 0,
    })
}

pub(crate) fn take<'a>(
    view: &mut View<'a>,
    len: usize,
    field: &'static str,
) -> Result<&'a [u8], CodecError> {
    Ok(view.req_take(len).map_err(|error| error.during(field))?)
}

/// Takes `len` bytes as a bounded window over the same space.
///
/// The take is the whole bounds proof: a `len` the window does not hold is the
/// located truncation this states, and the window it returns is exactly the
/// bytes it advanced over, so no caller recomputes the range.
pub(crate) fn take_child<'a>(
    view: &mut View<'a>,
    len: usize,
    field: &'static str,
) -> Result<View<'a>, CodecError> {
    Ok(view
        .req_take_child(len)
        .map_err(|error| error.during(field))?)
}

/// Returns a copy of `view` positioned at an absolute position in its space.
///
/// A position the window does not hold is the truncation the caller was about
/// to report from the read itself, so it states the same field and the
/// position it could not reach.
pub(crate) fn at<'a>(
    view: View<'a>,
    position: usize,
    field: &'static str,
) -> Result<View<'a>, CodecError> {
    let mut view = view;
    match view.seek(position) {
        Some(()) => Ok(view),
        None => Err(CodecError::truncated(view.location_at(position), field)),
    }
}

#[cfg(test)]
mod tests {
    use super::{array, at, u16_array};
    use crate::test_support::truncation::located_truncation;
    use cadmpeg_core::decode::View;

    #[test]
    fn a_short_array_read_names_the_array_start_and_a_short_element_read_names_the_element() {
        let bytes = [0_u8; 6];
        let mut view = View::over_retained(&bytes);
        assert_eq!(
            located_truncation(array::<8>(&mut view, "identifier")),
            "Truncated identifier at offset 0"
        );
        assert_eq!(view.read_len(), 0, "a refused array read does not advance");

        let mut view = View::over_retained(&bytes);
        assert_eq!(
            located_truncation(u16_array::<4>(&mut view, "prefix")),
            "Truncated prefix at offset 6"
        );
    }

    #[test]
    fn a_position_the_window_does_not_hold_is_a_located_truncation() {
        let bytes = [0_u8; 6];
        let view = View::over_retained(&bytes);
        assert_eq!(
            located_truncation(at(view, 7, "section size")),
            "Truncated section size at offset 7"
        );
        assert_eq!(
            at(view, 4, "section size").ok().map(View::read_len),
            Some(4)
        );
    }
}
