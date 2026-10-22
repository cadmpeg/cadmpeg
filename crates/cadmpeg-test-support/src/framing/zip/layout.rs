// SPDX-License-Identifier: Apache-2.0
//! ZIP32 directory admission from actual fixed headers and length words.

use cadmpeg_core::decode::View;

#[derive(Clone, Copy)]
pub(super) struct Directory {
    pub(super) start: usize,
    pub(super) end_record: usize,
    pub(super) count: usize,
    pub(super) shift: i128,
}

pub(super) fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    View::u16_le_at(bytes, at).ok_or_else(|| "cut-short ZIP u16 field".into())
}
pub(super) fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    View::u32_le_at(bytes, at).ok_or_else(|| "cut-short ZIP u32 field".into())
}
pub(super) fn signature(bytes: &[u8], at: usize, signature: &[u8; 4]) -> bool {
    at.checked_add(4).and_then(|end| bytes.get(at..end)) == Some(signature)
}
pub(super) fn add(at: usize, length: usize) -> Result<usize, String> {
    at.checked_add(length)
        .ok_or_else(|| "ZIP source offset overflows".into())
}
pub(super) fn size(value: u32) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| "ZIP size exceeds usize".into())
}

fn candidate(bytes: &[u8], end_record: usize) -> Option<Directory> {
    let count = u16_at(bytes, end_record.checked_add(10)?).ok()?;
    if u16_at(bytes, end_record.checked_add(4)?).ok()? != 0
        || u16_at(bytes, end_record.checked_add(6)?).ok()? != 0
        || u16_at(bytes, end_record.checked_add(8)?).ok()? != count
        || count == u16::MAX
    {
        return None;
    }
    let directory_size = u32_at(bytes, end_record.checked_add(12)?).ok()?;
    let declared = u32_at(bytes, end_record.checked_add(16)?).ok()?;
    if directory_size == u32::MAX || declared == u32::MAX {
        return None;
    }
    let start = end_record.checked_sub(size(directory_size).ok()?)?;
    let mut at = start;
    for _ in 0..count {
        if !signature(bytes, at, b"PK\x01\x02") {
            return None;
        }
        let lengths = [28, 30, 32]
            .into_iter()
            .try_fold(46_usize, |length, field| {
                length.checked_add(usize::from(u16_at(bytes, at.checked_add(field)?).ok()?))
            })?;
        at = at.checked_add(lengths)?;
        if at > end_record {
            return None;
        }
    }
    if at != end_record {
        return None;
    }
    Some(Directory {
        start,
        end_record,
        count: usize::from(count),
        shift: i128::try_from(start).ok()? - i128::from(declared),
    })
}

pub(super) fn directory(bytes: &[u8]) -> Result<Directory, String> {
    let mut found = None;
    let search_len = bytes.len().min(65_557);
    let begin = bytes
        .len()
        .checked_sub(search_len)
        .ok_or("ZIP end-record search offset underflows")?;
    for at in begin..bytes.len() {
        if !signature(bytes, at, b"PK\x05\x06") {
            continue;
        }
        let Some(comment_at) = at.checked_add(20) else {
            continue;
        };
        let Ok(comment) = u16_at(bytes, comment_at) else {
            continue;
        };
        if at
            .checked_add(22)
            .and_then(|end| end.checked_add(usize::from(comment)))
            != Some(bytes.len())
        {
            continue;
        }
        if let Some(candidate) = candidate(bytes, at) {
            if found.replace(candidate).is_some() {
                return Err("ambiguous ZIP end records frame two directories".into());
            }
        }
    }
    found.ok_or_else(|| "no complete single-disk ZIP32 directory; ZIP64 and digital-signature directories need their own framer".into())
}
