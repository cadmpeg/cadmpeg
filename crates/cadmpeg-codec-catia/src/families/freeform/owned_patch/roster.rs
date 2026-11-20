// SPDX-License-Identifier: Apache-2.0
//! Source-local membership of a wide packed face roster.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::layout::consolidated_wide_face_roster_prefix as prefix;
use crate::wire::bytes::{allocation_reference, compact_int};
use crate::wire::records::{
    parse_consolidated_record, records_are_contiguous, ConsolidatedFamily as Family,
    ConsolidatedFrameFlag as Flag, ConsolidatedRecord, SourceExtent,
};

pub(super) type RecordKey = (usize, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Member {
    pub(super) body: RecordKey,
    pub(super) forward: bool,
}

pub(super) type SheetMembers = BTreeMap<RecordKey, Vec<Member>>;
pub(super) type BodyRoots = BTreeMap<RecordKey, BTreeSet<usize>>;

pub(super) fn fallback_members(
    key: RecordKey,
    face: usize,
    legacy: Member,
    sheets: &SheetMembers,
    recovered: &BTreeSet<(RecordKey, usize)>,
) -> Vec<Member> {
    sheets
        .get(&key)
        .map_or(std::slice::from_ref(&legacy), Vec::as_slice)
        .iter()
        .copied()
        .filter(|member| !recovered.contains(&(member.body, face)))
        .collect()
}

pub(super) fn payload<'a>(bytes: &'a [u8], frame: &ConsolidatedRecord) -> Option<&'a [u8]> {
    bytes.get(frame.payload()?)
}

fn is_frame(frame: &ConsolidatedRecord, family: Family, class: u8) -> bool {
    frame.family() == family && frame.class() == class && frame.flag() == Flag::Flag03
}

pub(super) fn wide_roster(bytes: &[u8], flags: u8) -> Option<(Vec<usize>, &[u8])> {
    if *bytes.get(prefix::MARKER)? != 0x18 {
        return None;
    }
    let count = usize::from(View::u16_le_at(bytes, prefix::FACE_COUNT)?);
    let mut view = View::over_retained(bytes.get(prefix::LEN..)?);
    if count == 0 || view.u8()? != 0x03 {
        return None;
    }
    view.counted(u64::try_from(count - 1).ok()?, 2)?;
    let mut refs = Vec::new();
    refs.push(0);
    for _ in 1..count {
        let allocation = match view.u8()? {
            0x06 => u32::from(view.u8()?),
            0x0a => u32::from(view.u16_le()?),
            0x0e => {
                let low = u32::from(view.u16_le()?);
                low | (u32::from(view.u8()?) << 16)
            }
            _ => return None,
        };
        let allocation = usize::try_from(allocation).ok()?;
        if allocation <= *refs.last()? {
            return None;
        }
        refs.push(allocation);
    }
    Some((refs, roster_bits(view, count, flags)?))
}

pub(super) fn sheet_references(
    bytes: &[u8],
    family: Family,
) -> Option<(Vec<crate::wire::bytes::AllocationReference>, &[u8])> {
    let (count, mut at) = match (*bytes.first()?, family) {
        (0x18, Family::A) => (usize::from(View::u16_le_at(bytes, 1)?), 3),
        (0x08, Family::A) => (usize::from(*bytes.get(1)?), 2),
        (lead, _) => (usize::from(lead.checked_sub(0x80)?), 1),
    };
    if count == 0 {
        return None;
    }
    View::over_retained(bytes.get(at..)?).counted(u64::try_from(count).ok()?, 1)?;
    let references = (0..count)
        .map(|_| allocation_reference(bytes, &mut at))
        .collect::<Option<Vec<_>>>()?;
    Some((
        references,
        roster_bits(View::over_retained(bytes.get(at..)?), count, 3)?,
    ))
}

fn roster_bits(mut view: View<'_>, count: usize, flags: u8) -> Option<&[u8]> {
    if view.u8()? != 0 {
        return None;
    }
    let bits = view.take((count + 4).div_ceil(8))?;
    let used = (count + 4) % 8;
    if bits[0] & 0x0f != flags
        || (used != 0 && bits.last()? >> used != 0)
        || view.u8()? != 0x03
        || view.remaining() != 0
    {
        return None;
    }
    Some(bits)
}

pub(super) fn allocation_width(bytes: &[u8], frame: &ConsolidatedRecord) -> Option<usize> {
    let end = frame.payload()?.start;
    let width = usize::from(u8::from(frame.width()));
    let token = bytes.get(end.checked_sub(width)?..end)?;
    let mut at = 0;
    let value = usize::try_from(compact_int(token, &mut at)?).ok()?;
    (at == width && value > 0).then_some(value)
}

fn walk(
    bytes: &[u8],
    root: usize,
    limit: usize,
    refs: &[usize],
    bits: &[u8],
) -> Option<Vec<(usize, bool)>> {
    let mut position = root;
    let mut allocation = 0usize;
    let mut faces = Vec::new();
    for (slot, &target) in refs.iter().enumerate() {
        loop {
            let frame = parse_consolidated_record(bytes, position, limit)?;
            if is_frame(&frame, Family::B, 0x65) || allocation > target {
                return None;
            }
            let width = allocation_width(bytes, &frame)?;
            let start = position;
            position = frame.range()?.end;
            let first_slot = allocation;
            allocation = allocation.checked_add(width)?;
            if first_slot == target {
                if !is_frame(&frame, Family::B, 0x5f) || width != 1 {
                    return None;
                }
                let bit = slot + 4;
                faces.push((start, (bits[bit / 8] >> (bit % 8)) & 1 != 0));
                break;
            }
        }
    }
    Some(faces)
}

pub(super) fn logical_bytes<'a>(
    ctx: &DecodeContext<'_>,
    data: &'a [u8],
    extents: &[SourceExtent],
) -> Result<Cow<'a, [u8]>, CodecError> {
    if let [extent] = extents {
        return Ok(Cow::Borrowed(&data[extent.range()]));
    }
    let length = extents.iter().try_fold(0usize, |size, extent| {
        size.checked_add(extent.range().len())
            .ok_or_else(|| CodecError::malformed("CATIA roster source length overflow"))
    })?;
    let mut bytes = ctx.alloc_filled(length, 0, "CATIA face roster source")?;
    let mut at = 0;
    for extent in extents {
        let range = extent.range();
        bytes[at..at + range.len()].copy_from_slice(&data[range.clone()]);
        at += range.len();
    }
    Ok(Cow::Owned(bytes))
}

pub(super) fn legacy_body(group: &[ConsolidatedRecord], data: &[u8]) -> bool {
    let [body, shell, roster] = group else {
        return false;
    };
    is_frame(body, Family::B, 0x65)
        && is_frame(shell, Family::B, 0x60)
        && is_frame(roster, Family::A, 0x61)
        && [body, shell, roster]
            .iter()
            .all(|frame| u8::from(frame.width()) == 1 && frame.header_token() == 5)
        && payload(data, body) == Some(&[0x81, 0x03, 0x05, 0x0d])
        && payload(data, shell) == Some(&[0x81, 0x03])
        && records_are_contiguous(group)
}

pub(super) fn members(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    records: &[ConsolidatedRecord],
) -> Result<BTreeMap<RecordKey, Member>, CodecError> {
    let mut roots: BTreeMap<usize, Vec<(&ConsolidatedRecord, &ConsolidatedRecord)>> =
        BTreeMap::new();
    for group in records.windows(3) {
        if legacy_body(group, data) {
            let body = &group[0];
            let roster = &group[2];
            roots
                .entry(body.source_index())
                .or_default()
                .push((body, roster));
        }
    }
    let mut result = BTreeMap::new();
    for (source, roots) in roots {
        let bytes = logical_bytes(ctx, data, &sources[source])?;
        for (index, &(body, roster)) in roots.iter().enumerate() {
            let Some((refs, bits)) = payload(data, roster).and_then(|bytes| wide_roster(bytes, 1))
            else {
                continue;
            };
            let limit = roots
                .get(index + 1)
                .map_or(bytes.len(), |(next, _)| next.source_range().start);
            let Some(faces) = walk(&bytes, roster.source_range().end, limit, &refs, bits) else {
                continue;
            };
            for (face, forward) in faces {
                result.insert(
                    (source, face),
                    Member {
                        body: (source, body.byte_offset()),
                        forward,
                    },
                );
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_recovery_retains_each_unrecovered_native_owner() {
        let legacy = Member {
            body: (0, 10),
            forward: false,
        };
        let first = Member {
            body: (0, 20),
            forward: true,
        };
        let second = Member {
            body: (0, 30),
            forward: false,
        };
        let sheets = [((0, 100), vec![first, second])].into();
        let recovered = [((0, 20), 200)].into();
        assert_eq!(
            fallback_members((0, 100), 200, legacy, &sheets, &recovered),
            vec![second]
        );
        assert_eq!(
            fallback_members((0, 101), 201, legacy, &sheets, &recovered),
            vec![legacy]
        );
        assert_eq!(
            fallback_members((0, 100), 200, legacy, &sheets, &BTreeSet::new()),
            vec![first, second]
        );
    }

    #[test]
    fn wide_roster_uses_allocation_width_and_member_bits_across_extents() {
        let header = [
            0xb2, 0x03, 0x65, 4, 5, 0x81, 0x03, 0x05, 0x0d, 0xb2, 0x03, 0x60, 2, 5, 0x81, 0x03,
            0xa5, 0x03, 0x61, 9, 0, 0, 0, 5, 0x18, 2, 0, 3, 0x06, 4, 0, 0x21, 3,
        ];
        let face = [0xb2, 0x03, 0x5f, 4, 5, 0x82, 0x07, 0x03, 0x05];
        let multi = [0xb2, 0x03, 0x25, 1, 0x0d, 0];
        let mut bytes = [header.as_slice(), &face, &multi, &face].concat();
        let last = header.len() + face.len() + multi.len();
        let split = header.len() + face.len();
        bytes.splice(split..split, [0xff; 7]);
        let sources = vec![vec![
            SourceExtent::within(&bytes, 0, split).expect("valid fixture roster"),
            SourceExtent::within(&bytes, split + 7, bytes.len()).expect("valid fixture roster"),
        ]];
        let records = crate::wire::records::consolidated_records_in_sources(&bytes, &sources)
            .expect("valid fixture roster");
        crate::test_support::with_service_context(|ctx| {
            let found = members(ctx, &bytes, &sources, &records).expect("valid fixture roster");
            assert_eq!(found.len(), 2);
            assert_eq!(found[&(0, header.len())].body, (0, 0));
            assert!(!found[&(0, header.len())].forward);
            assert!(found[&(0, last)].forward);

            let mut bad = bytes.clone();
            bad[29] = 3;
            assert!(members(ctx, &bad, &sources, &records)
                .expect("valid fixture roster")
                .is_empty());
        });
    }
}
