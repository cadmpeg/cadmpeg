// SPDX-License-Identifier: Apache-2.0
//! Complete source-frame admission for compact-index column rows.

use super::{IndexRow, LinkedRow, TargetRow};
use crate::om::compact::LocatedCompactIndex;
use crate::om::{color::PaletteIndex, discriminators};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

fn push_row<T>(
    ctx: &DecodeContext<'_>,
    rows: &mut Vec<T>,
    row: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, operation)?;
    ctx.charge_retained(u64_from_index(std::mem::size_of::<T>()), operation)?;
    rows.try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    rows.push(row);
    Ok(())
}

/// Decode complete self-framed index rows from contiguous column storage.
pub(crate) fn index_rows(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<IndexRow>, CodecError> {
    use super::{INDEX_MIDDLE as MIDDLE, INDEX_PREFIX as PREFIX, INDEX_SUFFIX as SUFFIX};
    let mut rows = Vec::new();
    ctx.charge_work(u64_from_index(bytes.len()), "scan NX index rows")?;
    let mut start = 0;
    while start + PREFIX.len() <= bytes.len() {
        if bytes.get(start..start + PREFIX.len()) != Some(&PREFIX) {
            start += 1;
            continue;
        }
        let first_index_offset = start + PREFIX.len();
        let Some(first_token) = LocatedCompactIndex::read(bytes, first_index_offset) else {
            start += 1;
            continue;
        };
        let marker = first_token.offset + first_token.atom.raw().len();
        if bytes.get(marker..marker + 2) != Some(&MIDDLE[..2]) {
            start += 1;
            continue;
        }
        let Some(flag) = bytes
            .get(marker + 2)
            .copied()
            .and_then(|value| discriminators::LinkedIndexFlag::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let mut at = marker + 3;
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        if let Some(row) = IndexRow::<(), usize>::new(
            first_token.atom,
            flag,
            index_tokens.map(|token| token.atom.into()),
            start,
        ) {
            push_row(ctx, &mut rows, row, "NX index rows")?;
        }
        start = end;
    }
    Ok(rows)
}

/// Decode complete linked index rows from contiguous column storage.
pub(crate) fn linked_rows(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<LinkedRow>, CodecError> {
    use super::{ROW_SUFFIX as SUFFIX, TARGET_MIDDLE as MIDDLE};
    let mut rows = Vec::new();
    ctx.charge_work(u64_from_index(bytes.len()), "scan NX linked rows")?;
    let mut start = 0;
    while start + 2 <= bytes.len() {
        if bytes.get(start..start + 2) != Some(&super::LINKED_PREFIX) {
            start += 1;
            continue;
        }
        let first_offset = start + 2;
        let Some(first_token) = LocatedCompactIndex::read(bytes, first_offset) else {
            start += 1;
            continue;
        };
        let marker = first_token.offset + first_token.atom.raw().len();
        if bytes.get(marker..marker + 2) != Some(&super::LINKED_MIDDLE) {
            start += 1;
            continue;
        }
        let Some(discriminator) = bytes
            .get(marker + 2)
            .copied()
            .and_then(|value| discriminators::LinkedIndexDiscriminator::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let target_offset = marker + 3;
        let Some(target_token) = LocatedCompactIndex::read(bytes, target_offset) else {
            start += 1;
            continue;
        };
        let mut at = target_token.offset + target_token.atom.raw().len();
        if bytes.get(at..at + MIDDLE.len()) != Some(&MIDDLE) {
            start += 1;
            continue;
        }
        at += MIDDLE.len();
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
        if bytes.get(at..at + 2) != Some(&[0x00, 0x47]) {
            start += 1;
            continue;
        }
        let Some(flag) = bytes
            .get(at + 2)
            .copied()
            .and_then(|value| discriminators::LinkedIndexFlag::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let Some(mode) = bytes
            .get(at + 3)
            .copied()
            .and_then(|value| discriminators::IndexRowMode::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(4 + SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at + 4..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        if let Some(row) = LinkedRow::<(), usize>::new(
            first_token.atom,
            discriminator,
            target_token.atom.into(),
            index_tokens.map(|token| token.atom.into()),
            flag,
            mode,
            start,
        ) {
            push_row(ctx, &mut rows, row, "NX linked rows")?;
        }
        start = end;
    }
    Ok(rows)
}

/// Decode complete target-index rows from contiguous column storage.
pub(crate) fn target_rows(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Vec<TargetRow>, CodecError> {
    use super::TARGET_PREFIX as PREFIX;
    use super::{ROW_SUFFIX as SUFFIX, TARGET_MIDDLE as MIDDLE};
    let mut rows = Vec::new();
    ctx.charge_work(u64_from_index(bytes.len()), "scan NX target rows")?;
    let mut start = 0;
    while start + PREFIX.len() <= bytes.len() {
        if bytes.get(start..start + PREFIX.len()) != Some(&PREFIX) {
            start += 1;
            continue;
        }
        let target_offset = start + PREFIX.len();
        let Some(target_token) = LocatedCompactIndex::read(bytes, target_offset) else {
            start += 1;
            continue;
        };
        let mut at = target_token.offset + target_token.atom.raw().len();
        if bytes.get(at..at + MIDDLE.len()) != Some(&MIDDLE) {
            start += 1;
            continue;
        }
        at += MIDDLE.len();
        let Some(index_tokens) = LocatedCompactIndex::read_array(bytes, &mut at) else {
            start += 1;
            continue;
        };
        if bytes.get(at..at + 3) != Some(&[0x00, 0x47, 0x03]) {
            start += 1;
            continue;
        }
        let Some(mode) = bytes
            .get(at + 3)
            .copied()
            .and_then(|value| discriminators::IndexRowMode::try_from(value).ok())
        else {
            start += 1;
            continue;
        };
        let Some(end) = at.checked_add(4 + SUFFIX.len()) else {
            start += 1;
            continue;
        };
        if bytes.get(at + 4..end) != Some(&SUFFIX) {
            start += 1;
            continue;
        }
        if let Some(row) = TargetRow::<(), usize>::new(
            target_token.atom.into(),
            index_tokens.map(|token| token.atom.into()),
            mode,
            start,
        ) {
            push_row(ctx, &mut rows, row, "NX target rows")?;
        }
        start = end;
    }
    Ok(rows)
}

pub(crate) fn preceding_color(bytes: &[u8], row_offset: usize) -> Option<PaletteIndex> {
    use super::ROW_SUFFIX as PRECEDING_SUFFIX;
    for width in [1, 2] {
        let Some(offset) = row_offset.checked_sub(width) else {
            continue;
        };
        let Some(prefix_offset) = offset.checked_sub(PRECEDING_SUFFIX.len()) else {
            continue;
        };
        if bytes.get(prefix_offset..offset) != Some(&PRECEDING_SUFFIX) {
            continue;
        }
        if let Some(color_index) = PaletteIndex::read_display(bytes.get(offset..row_offset)?) {
            return Some(color_index);
        }
    }
    None
}

#[cfg(test)]
mod linked_row_color_index_tests {
    use crate::om::column_row::scan::{linked_rows, preceding_color, target_rows};

    #[test]
    fn requires_the_complete_preceding_suffix() {
        let row_bytes = [
            0x02, 0x0b, 7, 0x93, 0x8c, 0x16, 2, 0xff, 0xff, 0x90, 0xfe, 3, 4, 5, 0, 0x47, 3, 4, 1,
            0xc0, 0x44, 4, 0,
        ];
        let mut bytes = [1, 0xc0, 0x44, 4, 0, 0x80, 201].to_vec();
        bytes.extend(row_bytes);
        let rows = crate::test_support::with_decode_context(|ctx| linked_rows(ctx, &bytes)).unwrap();
        let color = preceding_color(&bytes, rows[0].offset()).expect("complete prefix");
        assert_eq!(color.value(), 201);
        assert_eq!(color.display_raw(), [0x80, 201]);

        bytes[1] = 0;
        assert_eq!(preceding_color(&bytes, rows[0].offset()), None);
    }

    #[test]
    fn accepts_the_same_prefix_for_a_target_index_row() {
        let row_bytes = [
            0x02, 0x01, 0x01, 0x01, 0x16, 2, 0xff, 0xff, 0x90, 0xfe, 3, 4, 5, 0, 0x47, 3, 4, 1,
            0xc0, 0x44, 4, 0,
        ];
        let mut bytes = [1, 0xc0, 0x44, 4, 0, 0x80, 201].to_vec();
        bytes.extend(row_bytes);
        let rows = crate::test_support::with_decode_context(|ctx| target_rows(ctx, &bytes)).unwrap();
        let color = preceding_color(&bytes, rows[0].offset()).expect("complete prefix");
        assert_eq!(color.value(), 201);
        assert_eq!(color.display_raw(), [0x80, 201]);
    }
}

#[cfg(test)]
mod tests {
    fn index_row_limit_error(policy: &cadmpeg_core::decode::DecodePolicy) -> cadmpeg_core::CodecError {
        let bytes = b"\x2d\x02\x0b\x2a\x93\x8a\x03\x80\x18\x20\x20\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, policy)
            .unwrap();
        super::index_rows(&ctx, bytes).expect_err("index row resource refusal")
    }

    #[test]
    fn om_index_row_route_refuses_collection_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        assert!(matches!(index_row_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
    }

    #[test]
    fn om_index_row_route_refuses_retained_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;
        assert!(matches!(index_row_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
    }

    #[test]
    fn om_index_row_route_refuses_work_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_work_units = 0;
        assert!(matches!(index_row_limit_error(&policy), cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
    }

    #[test]
    fn om_offset_store_index_rows_require_complete_exact_frames() {
        let first =
            b"\x2d\x02\x0b\x2a\x93\x8a\x03\x80\x18\x20\x20\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
        let second = b"\x2d\x02\x0b\x83\xb6\x93\x8a\x07\x80\x18\x20\x80\x4d\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
        let mut bytes = b"prefix".to_vec();
        bytes.extend_from_slice(first);
        bytes.extend_from_slice(b"gap");
        bytes.extend_from_slice(second);

        let rows = crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::index_rows(ctx, &bytes)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].offset(), 6);
        assert_eq!(rows[0].first_index().atom.value(), 42);
        assert_eq!(rows[0].first_index().atom.raw(), [0x2a]);
        assert_eq!(u8::from(rows[0].flag()), 3);
        assert_eq!(
            rows[0]
                .indices()
                .map(|token| (token.atom.value(), token.offset)),
            [(24, 13), (32, 15), (32, 16), (65, 17)]
        );
        assert_eq!(
            rows[0].indices().map(|token| token.atom.raw().to_vec()),
            [vec![0x80, 0x18], vec![0x20], vec![0x20], vec![0x41]]
        );
        assert_eq!(rows[1].first_index().atom.value(), 950);
        assert_eq!(rows[1].first_index().atom.raw(), [0x83, 0xb6]);
        assert_eq!(u8::from(rows[1].flag()), 7);
        assert_eq!(
            rows[1]
                .indices()
                .map(|token| (token.atom.value(), token.offset)),
            [(24, 38), (32, 40), (77, 41), (65, 43)]
        );
        assert_eq!(
            rows[1].indices().map(|token| token.atom.raw().to_vec()),
            [vec![0x80, 0x18], vec![0x20], vec![0x80, 0x4d], vec![0x41]]
        );

        let mut null = first.to_vec();
        null[3] = 0xff;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::index_rows(ctx, &null)).unwrap().is_empty());
        let mut other_flag = first.to_vec();
        other_flag[6] = 0x04;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::index_rows(ctx, &other_flag)).unwrap().is_empty());
        let mut overlong = first.to_vec();
        overlong.insert(12, 0x01);
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::index_rows(ctx, &overlong)).unwrap().is_empty());
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::index_rows(ctx, &first[..first.len() - 1])).unwrap().is_empty());
    }

    #[test]
    fn om_offset_store_linked_index_rows_require_complete_exact_frames() {
        let row = b"\x02\x0b\x83\x93\x93\x8c\x16\x24\xff\xff\x90\xfe\x20\x20\x41\x00\x47\x03\x04\x01\xc0\x44\x04\x00";
        let rows = crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::linked_rows(ctx, row)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (
                rows[0].first_index().atom.value(),
                rows[0].first_index().offset
            ),
            (915, 2)
        );
        assert_eq!(rows[0].first_index().atom.raw(), [0x83, 0x93]);
        assert_eq!(u8::from(rows[0].discriminator()), 0x16);
        assert_eq!(
            (
                rows[0].target_index().atom.value(),
                rows[0].target_index().offset
            ),
            (36, 7)
        );
        assert_eq!(rows[0].target_index().atom.raw(), [0x24]);
        assert_eq!(
            rows[0]
                .indices()
                .map(|token| (token.atom.value(), token.offset)),
            [(32, 12), (32, 13), (65, 14)]
        );
        assert_eq!(
            rows[0].indices().map(|token| token.atom.raw().to_vec()),
            [vec![0x20], vec![0x20], vec![0x41]]
        );
        assert_eq!(u8::from(rows[0].flag()), 3);
        assert_eq!(u8::from(rows[0].mode()), 4);

        let mut null = row.to_vec();
        null[7] = 0xff;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::linked_rows(ctx, &null)).unwrap().is_empty());
        let mut discriminator = row.to_vec();
        discriminator[6] = 0x15;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::linked_rows(ctx, &discriminator)).unwrap().is_empty());
        let mut flag = row.to_vec();
        flag[17] = 0x04;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::linked_rows(ctx, &flag)).unwrap().is_empty());
        let mut mode = row.to_vec();
        mode[18] = 0x06;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::linked_rows(ctx, &mode)).unwrap().is_empty());
        let mut mode_seven = row.to_vec();
        mode_seven[18] = 0x07;
        assert_eq!(
            u8::from(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::linked_rows(ctx, &mode_seven)).unwrap()[0].mode()),
            7
        );
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::linked_rows(ctx, &row[..row.len() - 1])).unwrap().is_empty());
    }

    #[test]
    fn om_offset_store_target_index_rows_require_complete_exact_frames() {
        let row =
        b"\x02\x01\x01\x01\x16\x3e\xff\xff\x90\xfe\x1e\x20\x58\x00\x47\x03\x07\x01\xc0\x44\x04\x00";
        let rows = crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::target_rows(ctx, row)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (
                rows[0].target_index().atom.value(),
                rows[0].target_index().offset
            ),
            (62, 5)
        );
        assert_eq!(rows[0].target_index().atom.raw(), [0x3e]);
        assert_eq!(
            rows[0]
                .indices()
                .map(|token| (token.atom.value(), token.offset)),
            [(30, 10), (32, 11), (88, 12)]
        );
        assert_eq!(
            rows[0].indices().map(|token| token.atom.raw().to_vec()),
            [vec![0x1e], vec![0x20], vec![0x58]]
        );
        assert_eq!(u8::from(rows[0].mode()), 7);

        let mut null = row.to_vec();
        null[5] = 0xff;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::target_rows(ctx, &null)).unwrap().is_empty());
        let mut discriminator = row.to_vec();
        discriminator[4] = 0x17;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::target_rows(ctx, &discriminator)).unwrap().is_empty());
        let mut suffix = row.to_vec();
        suffix[16] = 0x03;
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::target_rows(ctx, &suffix)).unwrap().is_empty());
        let mut mode_four = row.to_vec();
        mode_four[16] = 0x04;
        assert_eq!(
            u8::from(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::target_rows(ctx, &mode_four)).unwrap()[0].mode()),
            4
        );
        assert!(crate::test_support::with_decode_context(|ctx| crate::om::column_row::scan::target_rows(ctx, &row[..row.len() - 1])).unwrap().is_empty());
    }
}
