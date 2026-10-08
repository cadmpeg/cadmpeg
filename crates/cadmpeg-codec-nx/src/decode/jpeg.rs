// SPDX-License-Identifier: Apache-2.0
//! JPEG SOF dimension probe for NX raster payloads.

use cadmpeg_core::decode::View;

pub(crate) fn jpeg_dimensions(payload: &[u8]) -> Option<(u16, u16, u8, u8)> {
    if payload.get(..2)? != [0xff, 0xd8] {
        return None;
    }
    let mut offset = 2usize;
    while offset < payload.len() {
        while payload.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *payload.get(offset)?;
        offset += 1;
        if marker == 0xd9 || marker == 0xda {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(View::u16_be_at(payload, offset)?);
        if length < 2 {
            return None;
        }
        let segment_start = offset + 2;
        let segment_end = offset.checked_add(length)?;
        let segment = payload.get(segment_start..segment_end)?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let precision = *segment.first()?;
            let height = View::u16_be_at(segment, 1)?;
            let width = View::u16_be_at(segment, 3)?;
            let components = *segment.get(5)?;
            if width == 0
                || height == 0
                || components == 0
                || segment.len() != 6 + 3 * usize::from(components)
            {
                return None;
            }
            return Some((width, height, precision, components));
        }
        offset = segment_end;
    }
    None
}
