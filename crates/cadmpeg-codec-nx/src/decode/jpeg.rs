// SPDX-License-Identifier: Apache-2.0
//! JPEG SOF dimension probe for NX raster payloads.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(crate) fn jpeg_dimensions(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(u16, u16, u8, u8)>, CodecError> {
    if payload.get(..2) != Some(&[0xff, 0xd8]) {
        return Ok(None);
    }
    let mut offset = 2usize;
    while offset < payload.len() {
        ctx.charge_work(1, "NX JPEG segment scan")?;
        loop {
            ctx.charge_work(1, "NX JPEG fill byte scan")?;
            if payload.get(offset) != Some(&0xff) {
                break;
            }
            offset += 1;
        }
        let Some(&marker) = payload.get(offset) else {
            return Ok(None);
        };
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            return Ok(None);
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let Some(length) = View::u16_be_at(payload, offset).map(usize::from) else {
            return Ok(None);
        };
        if length < 2 {
            return Ok(None);
        }
        let segment_start = offset + 2;
        let Some(segment_end) = offset.checked_add(length) else {
            return Ok(None);
        };
        let Some(segment) = payload.get(segment_start..segment_end) else {
            return Ok(None);
        };
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let (Some(&precision), Some(height), Some(width), Some(&components)) = (
                segment.first(),
                View::u16_be_at(segment, 1),
                View::u16_be_at(segment, 3),
                segment.get(5),
            ) else {
                return Ok(None);
            };
            if width == 0
                || height == 0
                || components == 0
                || segment.len() != 6 + 3 * usize::from(components)
            {
                return Ok(None);
            }
            return Ok(Some((width, height, precision, components)));
        }
        offset = segment_end;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::jpeg_dimensions;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    fn assert_work_refusal(work: u64, operation: &str) {
        crate::test_support::with_decode_context_over(
            &[0xff, 0xd8, 0xff, 0xff, 0x01],
            |policy| policy.limits.max_work_units = work,
            |ctx| {
                let CodecError::ResourceLimit(limit) =
                    jpeg_dimensions(ctx, &[0xff, 0xd8, 0xff, 0xff, 0x01]).unwrap_err()
                else {
                    panic!("JPEG scan must propagate the resource refusal");
                };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, operation);
                let CodecError::ResourceLimit(later) =
                    ctx.charge_work(0, "later JPEG work").unwrap_err()
                else {
                    panic!("the context must retain its first refusal");
                };
                assert_eq!(limit, later);
            },
        );
    }

    #[test]
    fn jpeg_segment_scan_refuses_before_marker() {
        assert_work_refusal(0, "NX JPEG segment scan");
    }

    #[test]
    fn jpeg_fill_scan_refuses_before_next_byte() {
        assert_work_refusal(1, "NX JPEG fill byte scan");
    }

    #[test]
    fn jpeg_fill_scan_refuses_before_terminal_probe() {
        // One segment visit and two fill-byte probes precede the terminal probe.
        assert_work_refusal(3, "NX JPEG fill byte scan");
    }
}
