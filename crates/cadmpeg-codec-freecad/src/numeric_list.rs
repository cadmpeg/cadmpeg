// SPDX-License-Identifier: Apache-2.0
//! Source-backed `FreeCAD` numeric-list framing shared by product and attributes.

use crate::layout::link_array_side_entry_header as link_array;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

#[derive(Clone, Copy)]
pub(crate) enum RealWidth {
    Single,
    Double,
}

impl RealWidth {
    fn bytes(self) -> usize {
        match self {
            Self::Single => 4,
            Self::Double => 8,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RealPosition<'a> {
    view: View<'a>,
    offset: usize,
    width: RealWidth,
}

pub(crate) fn list_layout<'a, const N: usize>(
    view: View<'a>,
    name: &str,
) -> Result<impl ExactSizeIterator<Item = [RealPosition<'a>; N]>, CodecError> {
    let len = view.end() - view.start();
    if len < link_array::LEN {
        return Err(CodecError::malformed(format_args!("{name} is truncated")));
    }
    let mut head = view;
    head.seek(view.start())
        .ok_or_else(|| CodecError::malformed(format_args!("{name} header is out of bounds")))?;
    let count = usize::try_from(
        head.req_u32_le()
            .map_err(|error| CodecError::malformed(format_args!("link-array count: {error:?}")))?,
    )
    .map_err(CodecError::malformed)?;
    let encoded_len = |width: RealWidth| {
        count
            .checked_mul(N)
            .and_then(|count| count.checked_mul(width.bytes()))
            .and_then(|bytes| link_array::LEN.checked_add(bytes))
    };
    let width = if encoded_len(RealWidth::Double) == Some(len) {
        RealWidth::Double
    } else if encoded_len(RealWidth::Single) == Some(len) {
        RealWidth::Single
    } else {
        return Err(CodecError::malformed(format_args!(
            "{name} count {count} does not match {len} bytes"
        )));
    };
    Ok((0..count).map(move |index| {
        std::array::from_fn(|component| RealPosition {
            view,
            offset: view.start() + link_array::LEN + (index * N + component) * width.bytes(),
            width,
        })
    }))
}

pub(crate) fn read_real(position: RealPosition<'_>) -> Result<f64, CodecError> {
    let mut cursor = position.view;
    cursor
        .seek(position.offset)
        .ok_or_else(|| CodecError::malformed("real position is out of bounds"))?;
    match position.width {
        RealWidth::Double => cursor.req_f64_le().map_err(|error| {
            CodecError::malformed(format_args!("link-array component: {error:?}"))
        }),
        RealWidth::Single => cursor.req_f32_le().map(f64::from).map_err(|error| {
            CodecError::malformed(format_args!("link-array component: {error:?}"))
        }),
    }
}
