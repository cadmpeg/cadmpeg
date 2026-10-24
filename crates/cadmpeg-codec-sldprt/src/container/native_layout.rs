// SPDX-License-Identifier: Apache-2.0
//! Admission of native cache ranges and ZIP directory ownership.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{bounded_len, index_from_u32, View};
use cadmpeg_core::CodecError;

use crate::layout::zip_central_header as central;
use crate::layout::zip_eocd_header as eocd;
use crate::layout::{footer_padding_body as footer, named_padding_header as named};

use super::{
    block_hdr, cache_hdr, nibble_swap_name, outer_hdr, read_block_frame, Block, CacheCell,
    DirectoryEntry, MARKER,
};

fn malformed() -> CodecError {
    CodecError::malformed("native ZIP directory or anchored cache-cell geometry is inconsistent")
}

fn block_end(bytes: &[u8], block: &Block) -> Option<usize> {
    read_block_frame(bytes, block.offset).map(|(_, _, end)| end)
}

pub(crate) struct Envelope {
    pub(crate) start: usize,
    pub(crate) root: usize,
    pub(crate) padding_bytes: usize,
    pub(crate) members: BTreeSet<usize>,
    pub(crate) entries: Vec<DirectoryEntry>,
}

fn central_end(
    bytes: &[u8],
    at: usize,
    blocks: &[Block],
    seen: &mut BTreeSet<usize>,
    local_limit: usize,
) -> Option<usize> {
    bytes.get(at..at.checked_add(central::LEN)?)?;
    if bytes.get(at + central::VERSION_NEEDED..at + central::VERSION_NEEDED + MARKER.len())
        != Some(MARKER.as_slice())
    {
        return None;
    }
    let name_len = usize::from(View::u16_le_at(bytes, at + central::NAME_LEN)?);
    let extra = usize::from(View::u16_le_at(bytes, at + central::EXTRA_LEN)?);
    let comment = usize::from(View::u16_le_at(bytes, at + central::COMMENT_LEN)?);
    let relative = index_from_u32(View::u32_le_at(bytes, at + central::LOCAL_OFFSET)?);
    let local = relative.checked_add(outer_hdr::LEN)?.checked_add(4)?;
    let block = blocks.iter().find(|block| block.offset == local)?;
    if local >= local_limit || block_end(bytes, block)? > local_limit {
        return None;
    }
    if !seen.insert(local)
        || extra != 0
        || comment != 0
        || View::u16_le_at(bytes, at + central::DISK_START) != Some(0)
    {
        return None;
    }
    let name_start = at.checked_add(central::LEN)?;
    let end = name_start.checked_add(name_len)?;
    let name = nibble_swap_name(bytes.get(name_start..end)?)?;
    if block.section.name() != Some(name.as_str())
        || bytes.get(at + central::VERSION_NEEDED..at + central::EXTRA_LEN + 2)
            != bytes.get(local..local.checked_add(block_hdr::LEN)?)
        || bytes.get(name_start..end)
            != bytes.get(
                local + block_hdr::LEN..local.checked_add(block_hdr::LEN)?.checked_add(name_len)?,
            )
    {
        return None;
    }
    Some(end)
}

/// Establish complete anchored tail ownership before selecting an EOCD.
fn tail_tiles(bytes: &[u8], end: usize, root: usize, count: u16) -> bool {
    if end == root {
        return root.checked_add(eocd::LEN) == Some(bytes.len());
    }
    let Some(first) = end.checked_add(2) else {
        return false;
    };
    let Some(last_marker) = bytes.len().checked_sub(named::LEN) else {
        return false;
    };
    let mut cells = Vec::new();
    for marker in first..=last_marker {
        if bytes.get(marker..marker + MARKER.len()) != Some(MARKER.as_slice())
            || View::u16_le_at(bytes, marker + named::EXTRA_LEN) != Some(0)
        {
            continue;
        }
        let Some(length) = View::u32_le_at(bytes, marker + named::L).filter(|n| *n > 0) else {
            continue;
        };
        if length.checked_mul(2) != View::u32_le_at(bytes, marker + named::TWO_L)
            || Some(length / 2) != View::u32_le_at(bytes, marker + named::HALF_L)
        {
            continue;
        }
        let Some(name_len) = View::u16_le_at(bytes, marker + named::NAME_LEN).filter(|n| *n > 0)
        else {
            continue;
        };
        let Some(record_end) = marker
            .checked_add(named::LEN)
            .and_then(|n| n.checked_add(usize::from(name_len)))
        else {
            continue;
        };
        if bytes
            .get(marker + named::LEN..record_end)
            .and_then(nibble_swap_name)
            .is_none()
        {
            continue;
        }
        cells.push((marker, index_from_u32(length), record_end));
    }
    let strides: BTreeSet<_> = cells
        .iter()
        .filter_map(|&(marker, length, record_end)| {
            let displacement = marker.checked_sub(first).filter(|n| *n > 0)?;
            if length % displacement != 0 {
                return None;
            }
            let stride = length / displacement;
            (first
                .checked_add(stride)
                .is_some_and(|end| record_end <= end && end <= bytes.len()))
            .then_some(stride)
        })
        .collect();
    let mut solutions = 0;
    for stride in strides {
        let mut at = first;
        let mut named_count = 0;
        while let Some(next) = at.checked_add(stride).filter(|next| *next <= bytes.len()) {
            let matching = cells
                .iter()
                .filter(|&&(marker, length, record_end)| {
                    marker
                        .checked_sub(at)
                        .filter(|n| *n > 0)
                        .and_then(|n| n.checked_mul(stride))
                        == Some(length)
                        && record_end <= next
                })
                .count();
            if matching != 1 {
                break;
            }
            named_count += 1;
            at = next;
        }
        if named_count == 0 {
            continue;
        }
        let Some(footer_start) = at.checked_sub(2).filter(|start| *start <= root) else {
            continue;
        };
        let ordinary = if footer_start == root {
            root.checked_add(eocd::LEN)
        } else {
            Some(footer_start)
        };
        let Some(ordinary) = ordinary else { continue };
        let mut footer_strides = BTreeSet::new();
        let Some(first_footer) = ordinary.checked_add(1) else {
            return false;
        };
        let Some(last_footer) = bytes.len().checked_sub(footer::LEN) else {
            return false;
        };
        for body in first_footer..=last_footer {
            let displacement = body - ordinary;
            let Some(five_l) = View::u32_le_at(bytes, body + footer::FIVE_L).map(index_from_u32)
            else {
                continue;
            };
            let Some(divisor) = displacement.checked_mul(5) else {
                continue;
            };
            if five_l == 0 || five_l % divisor != 0 {
                continue;
            }
            let q = five_l / divisor;
            if displacement
                .checked_add(footer::LEN)
                .is_some_and(|n| n <= q)
                && footer_fields(bytes, body, count, stride, displacement, q)
            {
                footer_strides.insert(q);
            }
        }
        for q in footer_strides {
            let mut cursor = footer_start;
            let mut roots = 0;
            while cursor < bytes.len() {
                if cursor == root {
                    roots += 1;
                    cursor += eocd::LEN;
                    continue;
                }
                let Some(next) = cursor.checked_add(q).filter(|next| *next <= bytes.len()) else {
                    break;
                };
                if cursor < root && root < next {
                    break;
                }
                let Some(first_body) = cursor.checked_add(1) else {
                    break;
                };
                let Some(last_body) = next.checked_sub(footer::LEN) else {
                    break;
                };
                let matching = (first_body..=last_body)
                    .filter(|body| footer_fields(bytes, *body, count, stride, *body - cursor, q))
                    .count();
                if matching != 1 {
                    break;
                }
                cursor = next;
            }
            if cursor == bytes.len() && roots == 1 {
                solutions += 1;
                if solutions > 1 {
                    return false;
                }
            }
        }
    }
    solutions == 1
}

fn footer_fields(
    bytes: &[u8],
    body: usize,
    count: u16,
    stride: usize,
    displacement: usize,
    q: usize,
) -> bool {
    View::u32_le_at(bytes, body + footer::DISK_FIELDS) == Some(0)
        && View::u16_le_at(bytes, body + footer::ENTRIES_DISK) == Some(count)
        && View::u16_le_at(bytes, body + footer::ENTRIES_TOTAL) == Some(count)
        && View::u32_le_at(bytes, body + footer::NAMED_STRIDE).map(index_from_u32) == Some(stride)
        && View::u32_le_at(bytes, body + footer::FIVE_L).map(index_from_u32)
            == displacement.checked_mul(q).and_then(|n| n.checked_mul(5))
        && View::u16_le_at(bytes, body + footer::COMMENT_LEN) == Some(0)
}

pub(crate) fn source_envelope(
    bytes: &[u8],
    blocks: &[Block],
) -> Result<Option<Envelope>, CodecError> {
    if blocks.is_empty() {
        return Ok(None);
    }
    let Some(last_central) = bytes.len().checked_sub(central::LEN) else {
        return Ok(None);
    };
    let mut declared = false;
    for start in outer_hdr::LEN..=last_central {
        if bytes
            .get(start + central::VERSION_NEEDED..start + central::VERSION_NEEDED + MARKER.len())
            != Some(MARKER.as_slice())
            || View::u16_le_at(bytes, start + central::VERSION_MADE_BY) != Some(0)
        {
            continue;
        }
        let Some(relative) =
            View::u32_le_at(bytes, start + central::LOCAL_OFFSET).map(index_from_u32)
        else {
            continue;
        };
        if relative
            .checked_add(outer_hdr::LEN)
            .and_then(|offset| offset.checked_add(4))
            .is_some_and(|offset| {
                offset < start && blocks.iter().any(|block| block.offset == offset)
            })
        {
            declared = true;
        }
    }
    let mut selected = None;
    let last_root = bytes.len().checked_sub(eocd::LEN).ok_or_else(malformed)?;
    for root in outer_hdr::LEN..=last_root {
        if View::u16_le_at(bytes, root + eocd::DISK_NUMBER) != Some(0)
            || View::u16_le_at(bytes, root + eocd::CENTRAL_DISK) != Some(0)
            || View::u16_le_at(bytes, root + eocd::COMMENT_LEN) != Some(0)
        {
            continue;
        }
        let Some(count) =
            View::u16_le_at(bytes, root + eocd::ENTRIES_TOTAL).filter(|count| *count > 0)
        else {
            continue;
        };
        if View::u16_le_at(bytes, root + eocd::ENTRIES_DISK) != Some(count)
            || usize::from(count) > blocks.len()
        {
            continue;
        }
        let Some(size) = View::u32_le_at(bytes, root + eocd::CENTRAL_SIZE).map(index_from_u32)
        else {
            continue;
        };
        let Some(offset) = View::u32_le_at(bytes, root + eocd::CENTRAL_OFFSET).map(index_from_u32)
        else {
            continue;
        };
        let Some(start) = offset.checked_add(outer_hdr::LEN) else {
            continue;
        };
        if start.checked_add(size) != Some(root)
            || start >= root
            || bounded_len(u64::from(count), central::LEN, root - start).is_none()
        {
            continue;
        }
        let mut seen = BTreeSet::new();
        let mut at = start;
        let mut entries = Vec::new();
        for _ in 0..count {
            let Some(end) =
                central_end(bytes, at, blocks, &mut seen, start).filter(|end| *end <= root)
            else {
                break;
            };
            let Some(entry) = full_directory_entry(bytes, at + central::VERSION_NEEDED, end) else {
                break;
            };
            entries.push(entry);
            at = end;
        }
        if entries.len() != usize::from(count) {
            continue;
        }
        let mut additional = seen.clone();
        if central_end(bytes, at, blocks, &mut additional, start).is_some_and(|end| end <= root) {
            continue;
        }
        if !tail_tiles(bytes, at, root, count) {
            continue;
        }
        if selected.is_some() {
            return Err(malformed());
        }
        selected = Some(Envelope {
            start,
            root,
            padding_bytes: root - at + bytes.len() - (root + eocd::LEN),
            members: seen,
            entries,
        });
    }
    if declared && selected.is_none() {
        return Err(malformed());
    }
    Ok(selected)
}

fn full_directory_entry(bytes: &[u8], marker: usize, end: usize) -> Option<DirectoryEntry> {
    let at = marker.checked_sub(central::VERSION_NEEDED)?;
    let name_start = at.checked_add(central::LEN)?;
    let descriptor = bytes
        .get(at + central::COMMENT_LEN..at + central::LEN)?
        .try_into()
        .ok()?;
    bytes.get(end..end.checked_add(6)?)?;
    Some(DirectoryEntry {
        offset: marker,
        type_id: View::u32_le_at(bytes, marker + block_hdr::TYPE_ID)?,
        size: View::u32_le_at(bytes, at + central::COMP_SZ)?,
        name: nibble_swap_name(bytes.get(name_start..end)?)?,
        descriptor,
    })
}

fn valid_stride(
    bytes: &[u8],
    blocks: &[Block],
    cells: &[CacheCell],
    limit: usize,
    stride: usize,
) -> bool {
    let mut previous_end = outer_hdr::LEN;
    for cell in cells {
        let anchor = blocks
            .iter()
            .filter_map(|block| block_end(bytes, block))
            .filter(|end| *end <= cell.offset)
            .max()
            .unwrap_or(outer_hdr::LEN);
        let base = anchor.max(previous_end);
        let displacement = index_from_u32(cell.logical_len) / stride;
        let Some(end) = base.checked_add(stride).filter(|end| *end <= limit) else {
            return false;
        };
        let Some(record_end) = cell
            .offset
            .checked_add(cache_hdr::LEN)
            .and_then(|at| at.checked_add(cell.name.len()))
        else {
            return false;
        };
        if base.checked_add(displacement) != Some(cell.offset)
            || record_end > end
            || blocks.iter().any(|block| {
                block_end(bytes, block)
                    .is_some_and(|block_end| block.offset < end && base < block_end)
            })
        {
            return false;
        }
        previous_end = end;
    }
    true
}

pub(super) fn validate(
    bytes: &[u8],
    blocks: &mut Vec<Block>,
    cells: &mut Vec<CacheCell>,
    directory: &mut Vec<DirectoryEntry>,
) -> Result<usize, CodecError> {
    let tail = source_envelope(bytes, blocks)?;
    let limit = tail.as_ref().map_or(bytes.len(), |tail| tail.start);
    cells.retain(|cell| cell.offset < limit);
    if let Some(tail) = tail {
        blocks.retain(|block| tail.members.contains(&block.offset));
        *directory = tail.entries;
    }
    let Some(first) = cells.first() else {
        return Ok(limit);
    };
    let mut candidates = BTreeSet::new();
    for displacement in 5..=14 {
        let logical = index_from_u32(first.logical_len);
        if logical.is_multiple_of(displacement) {
            let stride = logical / displacement;
            if stride > 0
                && cells.iter().all(|cell| {
                    let logical = index_from_u32(cell.logical_len);
                    logical.is_multiple_of(stride) && (5..=14).contains(&(logical / stride))
                })
            {
                candidates.insert(stride);
            }
        }
    }
    candidates.retain(|stride| valid_stride(bytes, blocks, cells, limit, *stride));
    if candidates.len() != 1 {
        return Err(malformed());
    }
    let Some(stride) = candidates.first().copied() else {
        return Err(malformed());
    };
    if !valid_stride(bytes, blocks, cells, limit, stride) {
        return Err(malformed());
    }
    let mut previous_end = outer_hdr::LEN;
    for cell in cells {
        let anchor = blocks
            .iter()
            .filter_map(|block| block_end(bytes, block))
            .filter(|end| *end <= cell.offset)
            .max()
            .unwrap_or(outer_hdr::LEN);
        let base = anchor.max(previous_end);
        let end = base.checked_add(stride).ok_or_else(malformed)?;
        cell.physical_range = Some(base..end);
        previous_end = end;
    }
    Ok(limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::container::{
        anchored_cache_cell, make_block, make_cache_cell, outer_header,
    };

    fn scan(source: &[u8]) -> Result<super::super::ContainerScan<'_>, CodecError> {
        let arena = Box::leak(Box::new(cadmpeg_core::decode::DecodeArena::new()));
        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            source,
            arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )?;
        super::super::scan(&ctx, root)
    }

    fn primary_grid(fill: u8) -> Vec<u8> {
        let mut source = outer_header();
        for displacement in [6, 6, 9, 12, 11] {
            source.extend(anchored_cache_cell(100, displacement, "PreviewPNG", fill));
        }
        source
    }

    fn native_directory(with_grid: bool, fill: u8) -> (Vec<u8>, usize) {
        let mut source = outer_header();
        let mut locals = Vec::new();
        for (name, payload) in [
            ("ordinary", b"payload".as_slice()),
            ("empty", b"".as_slice()),
        ] {
            let signature = source.len();
            source.extend([0x51, 0x52, 0x53, 0x54]);
            let marker = source.len();
            let block = make_block(0x1020_3040, name, payload);
            source.extend(&block);
            locals.push((signature, marker, name, block));
        }
        let central_start = source.len();
        for (signature, _, name, block) in &locals {
            source.extend([0x61, 0x62, 0x63, 0x64, 0, 0]);
            source.extend(&block[..block_hdr::LEN]);
            source.extend([0u8; 10]);
            source.extend(
                u32::try_from(signature - outer_hdr::LEN)
                    .unwrap()
                    .to_le_bytes(),
            );
            source.extend(name.bytes().map(|byte| byte.rotate_left(4)));
        }
        if with_grid {
            source.extend([fill; 2]);
            let base = source.len();
            let stride = 105usize;
            let displacement = 9usize;
            source.extend(std::iter::repeat_n(fill, displacement));
            source.extend(MARKER);
            source.extend(0x1020_3040u32.to_le_bytes());
            for value in [1890u32, 472, 945] {
                source.extend(value.to_le_bytes());
            }
            source.extend(1u16.to_le_bytes());
            source.extend(0u16.to_le_bytes());
            source.extend([0u8; 10]);
            source.extend(
                u32::try_from(locals.last().unwrap().0 - outer_hdr::LEN)
                    .unwrap()
                    .to_le_bytes(),
            );
            source.push(b'X'.rotate_left(4));
            source.extend(make_cache_cell(500, "X"));
            source.resize(base + stride, fill);
            source.truncate(source.len() - 2);
        }
        let root = source.len();
        source.extend([0x71, 0x72, 0x73, 0x74]);
        source.extend([0u8; 4]);
        source.extend(2u16.to_le_bytes());
        source.extend(2u16.to_le_bytes());
        source.extend(u32::try_from(root - central_start).unwrap().to_le_bytes());
        source.extend(
            u32::try_from(central_start - outer_hdr::LEN)
                .unwrap()
                .to_le_bytes(),
        );
        source.extend(0u16.to_le_bytes());
        if with_grid {
            let base = source.len();
            source.extend([fill; 5]);
            source.extend([0u8; 4]);
            source.extend(2u16.to_le_bytes());
            source.extend(2u16.to_le_bytes());
            source.extend(105u32.to_le_bytes());
            source.extend(1050u32.to_le_bytes());
            source.extend(0u16.to_le_bytes());
            source.resize(base + 42, fill);
        }
        (source, root)
    }

    #[test]
    fn anchored_grid_preserves_zero_and_ff_opaque_ranges() {
        for fill in [0, 0xff] {
            let source = primary_grid(fill);
            let parsed = scan(&source).unwrap();
            assert_eq!(parsed.cache_cells.len(), 5);
            assert_eq!(
                parsed
                    .cache_cells
                    .iter()
                    .map(|cell| cell.offset)
                    .collect::<Vec<_>>(),
                [14, 114, 217, 320, 419]
            );
            assert_eq!(parsed.source_image, source.as_slice());
        }
    }

    #[test]
    fn relationally_valid_one_byte_marker_shift_is_rejected() {
        let mut source = primary_grid(0);
        let record = source[217..253].to_vec();
        source[218..254].copy_from_slice(&record);
        source[217] = 0xaa;
        assert!(super::super::try_cache_cell(&source, 218).is_some());
        assert!(scan(&source).is_err());
    }

    #[test]
    fn validated_tail_owns_padding_and_both_directory_locators() {
        for with_grid in [false, true] {
            for fill in [0, 0xff] {
                let (source, _) = native_directory(with_grid, fill);
                let parsed = scan(&source).unwrap();
                assert_eq!(parsed.blocks.len(), 2);
                assert_eq!(parsed.directory.len(), 2);
                assert!(parsed.cache_cells.is_empty());
                assert_eq!(parsed.source_image, source.as_slice());
            }
        }
    }

    #[test]
    fn real_directory_count_size_and_offset_cannot_be_changed_independently() {
        for with_grid in [false, true] {
            let (source, root) = native_directory(with_grid, 0);
            for (offset, wide) in [
                (eocd::ENTRIES_DISK, false),
                (eocd::ENTRIES_TOTAL, false),
                (eocd::CENTRAL_SIZE, true),
                (eocd::CENTRAL_OFFSET, true),
            ] {
                let mut changed = source.clone();
                let at = root + offset;
                if wide {
                    let value = View::u32_le_at(&changed, at).unwrap();
                    changed[at..at + 4].copy_from_slice(&(value + 1).to_le_bytes());
                } else {
                    let value = View::u16_le_at(&changed, at).unwrap();
                    changed[at..at + 2].copy_from_slice(&(value + 1).to_le_bytes());
                }
                assert!(scan(&changed).is_err());
            }
        }
    }

    #[test]
    fn terminal_empty_frame_still_has_to_validate_before_becoming_an_anchor() {
        for offset in [block_hdr::CRC32, block_hdr::COMP_SZ, block_hdr::UNCOMP_SZ] {
            let mut source = primary_grid(0);
            let start = source.len();
            let block = make_block(1, "Contents/Empty", &[]);
            source.extend(block);
            let at = start + offset;
            let value = View::u32_le_at(&source, at).unwrap();
            source[at..at + 4].copy_from_slice(&(value + 1).to_le_bytes());
            source.push(0x45);
            assert!(scan(&source).is_err());
        }
    }
    #[test]
    fn directory_count_cannot_truncate_a_contiguous_verified_member_chain() {
        let (mut source, root) = native_directory(false, 0);
        source[root + eocd::ENTRIES_DISK..root + eocd::ENTRIES_DISK + 2]
            .copy_from_slice(&1u16.to_le_bytes());
        source[root + eocd::ENTRIES_TOTAL..root + eocd::ENTRIES_TOTAL + 2]
            .copy_from_slice(&1u16.to_le_bytes());
        assert!(scan(&source).is_err());
    }

    #[test]
    fn complete_header_patterns_in_owned_tail_residual_are_not_members() {
        for corrupt in [false, true] {
            let (mut source, root) = native_directory(true, 0);
            let mut embedded = make_block(7, "Fake", b"data");
            if corrupt {
                embedded[block_hdr::CRC32] ^= 1;
            }
            let at = root - 53;
            assert!(at + embedded.len() < root);
            source[at..at + embedded.len()].copy_from_slice(&embedded);
            let parsed = scan(&source).unwrap();
            assert_eq!(parsed.blocks.len(), 2);
            assert_eq!(parsed.directory.len(), 2);
            assert!(parsed.cache_cells.is_empty());
        }
    }
    #[test]
    fn eocd_shaped_bytes_inside_owned_named_residual_are_opaque() {
        for fill in [0, 0xff] {
            let (mut source, root) = native_directory(true, fill);
            let original_len = source.len();
            let fake = root - 26;
            let mut eocd_copy = source[root..root + eocd::LEN].to_vec();
            let start = index_from_u32(View::u32_le_at(&eocd_copy, eocd::CENTRAL_OFFSET).unwrap())
                + outer_hdr::LEN;
            eocd_copy[eocd::CENTRAL_SIZE..eocd::CENTRAL_SIZE + 4]
                .copy_from_slice(&u32::try_from(fake - start).unwrap().to_le_bytes());
            source[fake..fake + eocd::LEN].copy_from_slice(&eocd_copy);
            assert_eq!(source.len(), original_len);
            let parsed = scan(&source).unwrap();
            assert_eq!(parsed.blocks.len(), 2);
            assert_eq!(parsed.directory.len(), 2);
            assert_eq!(parsed.source_image, source.as_slice());
        }
    }

    #[test]
    fn named_and_footer_scale_relations_and_exact_eof_are_required() {
        let (source, root) = native_directory(true, 0);
        let marker = source[..root]
            .windows(MARKER.len())
            .rposition(|bytes| bytes == MARKER)
            .unwrap();
        // The final embedded primary-looking marker belongs to the named residual;
        // find the actual named header by its nonzero length scaling instead.
        let named_marker = (0..marker)
            .rev()
            .find(|at| View::u32_le_at(&source, *at + named::L) == Some(945))
            .unwrap();
        for at in [
            named_marker + named::TWO_L,
            root + eocd::LEN + 5 + footer::FIVE_L,
            root + eocd::LEN + 5 + footer::NAMED_STRIDE,
        ] {
            let mut changed = source.clone();
            changed[at] ^= 1;
            assert!(scan(&changed).is_err());
        }
        let mut changed = source;
        changed.push(0);
        assert!(scan(&changed).is_err());
    }
}
