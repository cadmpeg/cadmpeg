// SPDX-License-Identifier: Apache-2.0
//! ZIP directory admission from actual fixed headers and length words.

use cadmpeg_core::decode::View;

#[derive(Clone, Copy)]
pub(super) struct Directory {
    pub(super) start: usize,
    pub(super) end_record: usize,
    pub(super) central_end: usize,
    pub(super) zip64: Option<End64>,
    pub(super) signature: Option<DigitalSignature>,
    pub(super) count: usize,
    pub(super) shift: i128,
}

#[derive(Clone, Copy)]
pub(super) struct End64 {
    pub(super) start: usize,
    pub(super) locator: usize,
}

#[derive(Clone, Copy)]
pub(super) struct DigitalSignature {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) inside: bool,
}

struct Walk {
    start: usize,
    count: usize,
    central_end: usize,
    signature: Option<DigitalSignature>,
}

pub(super) fn u64_at(bytes: &[u8], at: usize) -> Result<u64, String> {
    View::u64_le_at(bytes, at).ok_or_else(|| "cut-short ZIP u64 field".into())
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
pub(super) fn size(value: u64) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| "ZIP size exceeds usize".into())
}

fn headers(bytes: &[u8], start: usize, count: usize, end: usize) -> Option<usize> {
    if count > end.checked_sub(start)? / 46 {
        return None;
    }
    let mut at = start;
    for _ in 0..count {
        if !signature(bytes, at, b"PK\x01\x02") {
            return None;
        }
        let length = [28, 30, 32]
            .into_iter()
            .try_fold(46_usize, |length, field| {
                length.checked_add(usize::from(u16_at(bytes, at.checked_add(field)?).ok()?))
            })?;
        at = at.checked_add(length)?;
        if at > end {
            return None;
        }
    }
    Some(at)
}

fn signature_ends(bytes: &[u8], start: usize, end: usize) -> bool {
    signature(bytes, start, b"PK\x05\x05")
        && start
            .checked_add(4)
            .and_then(|field| u16_at(bytes, field).ok())
            .and_then(|length| start.checked_add(6)?.checked_add(usize::from(length)))
            == Some(end)
}

fn walk(bytes: &[u8], end: usize, count: u64, length: u64) -> Option<Walk> {
    let length = usize::try_from(length).ok()?;
    let count = usize::try_from(count).ok()?;
    let canonical = end.checked_sub(length)?;
    let actual_headers = headers(bytes, canonical, count, end);
    if actual_headers == Some(end) && length != 0 {
        return Some(Walk {
            start: canonical,
            count,
            central_end: end,
            signature: None,
        });
    }
    if let Some(last) =
        actual_headers.filter(|last| *last < end && signature_ends(bytes, *last, end))
    {
        return Some(Walk {
            start: canonical,
            count,
            central_end: end,
            signature: Some(DigitalSignature {
                start: last,
                end,
                inside: true,
            }),
        });
    }
    let mut outside = None;
    let search_length = end.min(65_541);
    let begin = end.checked_sub(search_length)?;
    for at in begin..end {
        if !signature_ends(bytes, at, end) {
            continue;
        }
        let Some(start) = at.checked_sub(length) else {
            continue;
        };
        if headers(bytes, start, count, at) != Some(at) {
            continue;
        }
        let candidate = Walk {
            start,
            count,
            central_end: at,
            signature: Some(DigitalSignature {
                start: at,
                end,
                inside: false,
            }),
        };
        if outside.replace(candidate).is_some() {
            return None;
        }
    }
    if outside.is_some() {
        return outside;
    }
    (actual_headers == Some(end)).then_some(Walk {
        start: canonical,
        count,
        central_end: end,
        signature: None,
    })
}

fn zip64_candidate(bytes: &[u8], end_record: usize, locator: usize) -> Option<Directory> {
    if u32_at(bytes, locator.checked_add(4)?).ok()? != 0
        || u32_at(bytes, locator.checked_add(16)?).ok()? != 1
    {
        return None;
    }
    let declared_end = u64_at(bytes, locator.checked_add(8)?).ok()?;
    let mut direct = None;
    let mut fallback = None;
    for at in 0..locator {
        if !signature(bytes, at, b"PK\x06\x06") {
            continue;
        }
        let Some(length) = at
            .checked_add(4)
            .and_then(|field| u64_at(bytes, field).ok())
        else {
            continue;
        };
        if length < 44
            || usize::try_from(length)
                .ok()
                .and_then(|length| at.checked_add(12)?.checked_add(length))
                != Some(locator)
        {
            continue;
        }
        if u32_at(bytes, at.checked_add(16)?).ok()? != 0
            || u32_at(bytes, at.checked_add(20)?).ok()? != 0
        {
            continue;
        }
        let count = u64_at(bytes, at.checked_add(32)?).ok()?;
        if u64_at(bytes, at.checked_add(24)?).ok()? != count {
            continue;
        }
        let size = u64_at(bytes, at.checked_add(40)?).ok()?;
        let Some(walk) = walk(bytes, at, count, size) else {
            continue;
        };
        let declared = u64_at(bytes, at.checked_add(48)?).ok()?;
        let shift = i128::try_from(walk.start).ok()? - i128::from(declared);
        let result = Directory {
            start: walk.start,
            end_record,
            central_end: walk.central_end,
            zip64: Some(End64 { start: at, locator }),
            signature: walk.signature,
            count: walk.count,
            shift,
        };
        if i128::from(declared_end) + shift == i128::try_from(at).ok()? {
            if direct.replace(result).is_some() {
                return None;
            }
        }
        fallback = Some(result);
    }
    direct.or(fallback)
}

fn candidate(bytes: &[u8], end_record: usize) -> Option<Directory> {
    if u16_at(bytes, end_record.checked_add(4)?).ok()? != 0
        || u16_at(bytes, end_record.checked_add(6)?).ok()? != 0
    {
        return None;
    }
    if let Some(locator) = end_record
        .checked_sub(20)
        .filter(|at| signature(bytes, *at, b"PK\x06\x07"))
    {
        return zip64_candidate(bytes, end_record, locator);
    }
    let count = u16_at(bytes, end_record.checked_add(10)?).ok()?;
    if u16_at(bytes, end_record.checked_add(8)?).ok()? != count || count == u16::MAX {
        return None;
    }
    let size = u32_at(bytes, end_record.checked_add(12)?).ok()?;
    let declared = u32_at(bytes, end_record.checked_add(16)?).ok()?;
    if size == u32::MAX || declared == u32::MAX {
        return None;
    }
    let walk = walk(bytes, end_record, u64::from(count), u64::from(size))?;
    Some(Directory {
        start: walk.start,
        end_record,
        central_end: walk.central_end,
        zip64: None,
        signature: walk.signature,
        count: walk.count,
        shift: i128::try_from(walk.start).ok()? - i128::from(declared),
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
    found.ok_or_else(|| "no complete unambiguous single-disk ZIP directory".into())
}
