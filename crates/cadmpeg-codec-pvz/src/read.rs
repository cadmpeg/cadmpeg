// SPDX-License-Identifier: Apache-2.0
//! Field access inside a fixed-size record window.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

/// A reader positioned at `offset` bytes into `record`.
pub(crate) fn at(record: View<'_>, offset: usize) -> Result<View<'_>, CodecError> {
    let mut field = record;
    record
        .start()
        .checked_add(offset)
        .and_then(|position| field.seek(position))
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "field offset {offset} lies outside the {}-byte record at offset {}",
                record.window().len(),
                record.start()
            ))
        })?;
    Ok(field)
}

/// Takes the next `len` bytes of `view` as one record window.
pub(crate) fn record<'a>(view: &mut View<'a>, len: usize) -> Result<View<'a>, CodecError> {
    Ok(view.req_take_child(len)?)
}

/// Reads three consecutive little-endian `f32` values.
pub(crate) fn f32x3(view: &mut View<'_>) -> Result<[f32; 3], CodecError> {
    Ok([view.req_f32_le()?, view.req_f32_le()?, view.req_f32_le()?])
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::View;

    use super::{at, f32x3, record};

    #[test]
    fn fields_are_read_at_their_record_offset() {
        let bytes = [
            9, 9, 1, 0, 0, 0, 0, 0, 0, 0, 0x80, 0x3f, 0, 0, 0, 0x40, 0, 0, 0x40, 0x40,
        ];
        let mut view = View::over_retained(&bytes);
        view.req_take(2).unwrap();
        let header = record(&mut view, 6).unwrap();
        assert_eq!(at(header, 0).unwrap().req_u32_le().unwrap(), 1);
        assert!(at(header, 7).is_err());
        assert_eq!(f32x3(&mut view).unwrap(), [1.0, 2.0, 3.0]);
        assert!(record(&mut view, 1).is_err());
    }
}
