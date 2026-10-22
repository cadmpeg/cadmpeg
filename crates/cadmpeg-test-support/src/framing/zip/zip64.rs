// SPDX-License-Identifier: Apache-2.0
//! Member ZIP64 scalar order from the original sentinel fields.

use std::ops::Range;

use super::layout::{add, u16_at, u64_at};

pub(super) fn mask((uncompressed, compressed, offset): (u32, u32, u32)) -> u8 {
    u8::from(uncompressed == u32::MAX)
        | (u8::from(compressed == u32::MAX) << 1)
        | (u8::from(offset == u32::MAX) << 2)
}

pub(super) fn field(bytes: &[u8], extent: Range<usize>) -> Result<Option<Range<usize>>, String> {
    let mut at = extent.start;
    let mut found = None;
    while at < extent.end {
        let content = add(at, 4)?;
        let end = add(content, usize::from(u16_at(bytes, add(at, 2)?)?))?;
        if end > extent.end {
            return Err("ZIP extra field exceeds its declared block".into());
        }
        if u16_at(bytes, at)? == 1 && found.replace(content..end).is_some() {
            return Err("duplicate ZIP64 extra fields are ambiguous".into());
        }
        at = end;
    }
    Ok(found)
}

pub(super) fn resolve(
    bytes: &[u8],
    extent: Range<usize>,
    original: (u32, u32, u32),
) -> Result<(u64, u64, u64), String> {
    let required = mask(original);
    let (first, second, third) = original;
    let mut values = [u64::from(first), u64::from(second), u64::from(third)];
    if required != 0 {
        let source = field(bytes, extent)?.ok_or("ZIP64 sentinel has no actual extra field")?;
        let mut at = source.start;
        for (index, value) in values.iter_mut().enumerate() {
            if required & (1 << index) != 0 {
                if add(at, 8)? > source.end {
                    return Err("cut-short ZIP64 scalar in extra field".into());
                }
                *value = u64_at(bytes, at)?;
                at = add(at, 8)?;
            }
        }
    }
    Ok((values[0], values[1], values[2]))
}
