// SPDX-License-Identifier: Apache-2.0
//! Bounded decoder for the historical Unix `compress` (`.Z`) LZW stream.

use crate::layout::unix_compress_header as unix_compress;
use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, ExpandSpec, ExpandWriter, ScopedReservation,
};
use cadmpeg_core::CodecError;

const BLOCK_MODE: u8 = 0x80;
const CLEAR: u16 = 256;

pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    data: &[u8],
    expected_length: usize,
) -> Result<Option<Vec<u8>>, CodecError> {
    if data.get(unix_compress::MAGIC..unix_compress::FLAGS) != Some(&[0x1f, 0x9d]) {
        return Ok(None);
    }
    let Some(&flags) = data.get(unix_compress::FLAGS) else {
        return Ok(None);
    };
    let Some(rest) = data.get(unix_compress::LEN..) else {
        return Ok(None);
    };
    let max_bits = usize::from(flags & 0x1f);
    if !(9..=16).contains(&max_bits) || flags & !(BLOCK_MODE | 0x1f) != 0 {
        return Ok(None);
    }
    let block_mode = flags & BLOCK_MODE != 0;
    let dictionary_limit = 1usize << max_bits;
    let reservation =
        ctx.reserve_scoped(u64_from_index(expected_length), "inflate Creo TOC section")?;
    let mut output = ctx.begin_expand(ExpandSpec::Exact(u64_from_index(expected_length)))?;
    let dictionary_bytes = dictionary_limit * 4;
    let _dictionary_reservation = ctx.reserve_scoped(
        u64_from_index(dictionary_bytes),
        "decode Creo LZW dictionary",
    )?;
    let mut prefix = ctx.alloc_filled(dictionary_limit, 0u16, "creo LZW prefix slots")?;
    let mut suffix = ctx.alloc_filled(dictionary_limit, 0u8, "creo LZW suffix slots")?;
    for (value, slot) in suffix.iter_mut().take(256).enumerate() {
        let Ok(value) = u8::try_from(value) else {
            return Ok(None);
        };
        *slot = value;
    }

    let mut reader = CodeReader::new(rest, max_bits);
    let mut free_entry = if block_mode { 257usize } else { 256usize };
    let Some(first) = reader.next(free_entry, false) else {
        return Ok(None);
    };
    let first = usize::from(first);
    if first >= 256 {
        return Ok(None);
    }
    let mut old_code = first;
    let Ok(mut final_byte) = u8::try_from(first) else {
        return Ok(None);
    };
    if expected_length == 0 {
        return Ok(None);
    }
    output.write(&[final_byte])?;
    let mut written = 1;
    let mut stack = Vec::new();
    stack.try_reserve_exact(dictionary_limit).map_err(|_| {
        ctx.refuse_codec_limit(
            "allocate Creo LZW stack",
            u64_from_index(dictionary_limit),
            u64_from_index(dictionary_limit) + 1,
        )
    })?;

    if written == expected_length {
        return finish_expansion(ctx, output, reservation).map(Some);
    }

    while let Some(raw_code) = reader.next(free_entry, false) {
        ctx.charge_work(1, "decode Creo LZW code")?;
        if block_mode && raw_code == CLEAR {
            free_entry = 257;
            let Some(code) = reader.next(free_entry, true) else {
                break;
            };
            let code = usize::from(code);
            if code >= 256 {
                return Ok(None);
            }
            old_code = code;
            let Ok(byte) = u8::try_from(code) else {
                return Ok(None);
            };
            final_byte = byte;
            written += 1;
            if written > expected_length {
                return Ok(None);
            }
            output.write(&[final_byte])?;
            if written == expected_length {
                return finish_expansion(ctx, output, reservation).map(Some);
            }
            continue;
        }

        let input_code = usize::from(raw_code);
        let mut code = input_code;
        if code >= free_entry {
            if code != free_entry {
                return Ok(None);
            }
            if stack.len() == dictionary_limit {
                return Ok(None);
            }
            stack.push(final_byte);
            code = old_code;
        }
        while code >= 256 {
            ctx.charge_work(1, "decode Creo LZW dictionary chain")?;
            if code >= free_entry || code >= dictionary_limit {
                return Ok(None);
            }
            if stack.len() == dictionary_limit {
                return Ok(None);
            }
            stack.push(suffix[code]);
            code = usize::from(prefix[code]);
        }
        let Ok(byte) = u8::try_from(code) else {
            return Ok(None);
        };
        final_byte = byte;
        let Some(next_written) = written
            .checked_add(1)
            .and_then(|value| value.checked_add(stack.len()))
        else {
            return Ok(None);
        };
        if next_written > expected_length {
            return Ok(None);
        }
        output.write(&[final_byte])?;
        stack.reverse();
        output.write(&stack)?;
        stack.clear();
        written = next_written;
        if written == expected_length {
            return finish_expansion(ctx, output, reservation).map(Some);
        }

        if free_entry < dictionary_limit {
            let Ok(previous) = u16::try_from(old_code) else {
                return Ok(None);
            };
            prefix[free_entry] = previous;
            suffix[free_entry] = final_byte;
            free_entry += 1;
        }
        old_code = input_code;
    }

    if written == expected_length {
        finish_expansion(ctx, output, reservation).map(Some)
    } else {
        Ok(None)
    }
}

fn finish_expansion(
    ctx: &DecodeContext<'_>,
    output: ExpandWriter<'_, '_>,
    reservation: ScopedReservation<'_>,
) -> Result<Vec<u8>, CodecError> {
    let view = output.finalize()?;
    reservation.commit()?;
    ctx.copy_retained(view.window(), "retain Creo expanded section")
}

struct CodeReader<'a> {
    data: &'a [u8],
    cursor: usize,
    block: &'a [u8],
    bit_offset: usize,
    start_limit: usize,
    width: usize,
    max_bits: usize,
}

impl<'a> CodeReader<'a> {
    fn new(data: &'a [u8], max_bits: usize) -> Self {
        Self {
            data,
            cursor: 0,
            block: &[],
            bit_offset: 0,
            start_limit: 0,
            width: 9,
            max_bits,
        }
    }

    fn next(&mut self, free_entry: usize, clear: bool) -> Option<u16> {
        let max_code = (1usize << self.width) - 1;
        if clear {
            self.width = 9;
            self.block = &[];
            self.bit_offset = 0;
            self.start_limit = 0;
        } else if free_entry > max_code && self.width < self.max_bits {
            self.width += 1;
            self.block = &[];
            self.bit_offset = 0;
            self.start_limit = 0;
        }
        if self.bit_offset >= self.start_limit {
            if self.cursor >= self.data.len() {
                return None;
            }
            let end = self.cursor.saturating_add(self.width).min(self.data.len());
            self.block = &self.data[self.cursor..end];
            self.cursor = end;
            self.bit_offset = 0;
            self.start_limit = self
                .block
                .len()
                .checked_mul(8)?
                .checked_sub(self.width - 1)?;
            if self.start_limit == 0 {
                return None;
            }
        }
        let mut code = 0u16;
        for bit in 0..self.width {
            let source = self.bit_offset + bit;
            let value = (self.block[source / 8] >> (source % 8)) & 1;
            code |= u16::from(value) << bit;
        }
        self.bit_offset += self.width;
        Some(code)
    }
}

#[cfg(test)]
mod tests {
    fn codes(values: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0; values.len().saturating_mul(9).div_ceil(8)];
        for (index, value) in values.iter().copied().enumerate() {
            for bit in 0..9 {
                bytes[(index * 9 + bit) / 8] |= u8::try_from((value >> bit) & 1)
                    .expect("required invariant")
                    << ((index * 9 + bit) % 8);
            }
        }
        bytes
    }

    use super::CLEAR;

    fn decode(data: &[u8], expected_length: usize) -> Option<Vec<u8>> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
            .expect("test stream is admitted");
        super::decode(&ctx, data, expected_length)
            .expect("test stream stays within resource limits")
    }

    #[test]
    fn toc_expansion_refuses_before_materializing_output() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("small compressed input is admitted");
        let error = super::decode(&ctx, &stream, 3)
            .expect_err("three output bytes exceed the two-byte live reservation");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "inflate Creo TOC section"
        ));
    }

    #[test]
    fn toc_expansion_refuses_per_expansion_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_per_expand = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("small compressed input is admitted");
        let error = super::decode(&ctx, &stream, 3)
            .expect_err("three output bytes exceed the two-byte expansion limit");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::DecompressedBytes
                    && limit.operation == "begin_expand"
        ));
    }

    #[test]
    fn toc_expansions_share_the_cumulative_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_total = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("small compressed input is admitted");
        assert_eq!(
            super::decode(&ctx, &stream, 3).expect("first section is admitted"),
            Some(b"ABC".to_vec())
        );
        let error = super::decode(&ctx, &stream, 3)
            .expect_err("second section exceeds the remaining two bytes");
        assert!(matches!(
            error,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::DecompressedBytes
                    && limit.operation == "begin_expand"
        ));
    }

    #[test]
    fn decodes_literal_non_block_stream() {
        // Nine-bit LSB-first codes 65, 66, 67.
        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        assert_eq!(decode(&stream, 3), Some(b"ABC".to_vec()));
        assert_eq!(decode(&stream, 4), None);
    }

    #[test]
    fn rejects_invalid_header_flags() {
        assert_eq!(decode(&[0x1f, 0x9d, 0x08], 0), None);
        assert_eq!(decode(&[0x1f, 0x9d, 0x30], 0), None);
    }

    #[test]
    fn rejects_truncated_code_block() {
        assert_eq!(decode(&[0x1f, 0x9d, 0x09, 0x00], 1), None);
    }

    #[test]
    fn non_block_mode_starts_with_literal_dictionary_slot_256() {
        let mut stream = vec![0x1f, 0x9d, 0x10];
        stream.extend(codes(&[u16::from(b'A'), u16::from(b'A'), 256]));
        assert_eq!(decode(&stream, 4), Some(b"AAAA".to_vec()));
    }

    #[test]
    fn non_block_mode_grows_width_after_filling_the_nine_bit_dictionary() {
        let values = std::iter::once(u16::from(b'A'))
            .chain(std::iter::repeat_n(u16::from(b'A'), 257))
            .collect::<Vec<_>>();
        let mut packed = Vec::new();
        for (width, block) in [(9, &values[..257]), (10, &values[257..])] {
            for chunk in block.chunks(8) {
                let block_offset = packed.len();
                packed.resize(block_offset + width, 0);
                for (index, value) in chunk.iter().copied().enumerate() {
                    for bit in 0..width {
                        let bit_offset = index * width + bit;
                        packed[block_offset + bit_offset / 8] |= u8::try_from((value >> bit) & 1)
                            .expect("required invariant")
                            << (bit_offset % 8);
                    }
                }
            }
        }
        let mut stream = vec![0x1f, 0x9d, 0x10];
        stream.extend(packed);
        assert_eq!(
            decode(&stream, values.len()),
            Some(vec![b'A'; values.len()])
        );
    }

    #[test]
    fn decodes_block_mode_dictionary_references() {
        let stream = [
            0x1f, 0x9d, 0x90, 0x54, 0x9e, 0x08, 0x29, 0xf2, 0x44, 0x8a, 0x93, 0x27, 0x54, 0x02,
            0x0e, 0x2c, 0xa8, 0x90, 0xa0, 0x41, 0x84, 0x0a, 0x00,
        ];
        assert_eq!(
            decode(&stream, 25),
            Some(b"TOBEORNOTTOBEORTOBEORNOT\n".to_vec())
        );
    }

    #[test]
    fn block_mode_clear_reserves_the_clear_code() {
        let mut stream = vec![0x1f, 0x9d, 0x90];
        let mut first_block = codes(&[u16::from(b'A'), CLEAR]);
        first_block.resize(9, 0);
        stream.extend(first_block);
        stream.extend(codes(&[u16::from(b'B'), u16::from(b'C'), 257]));
        assert_eq!(decode(&stream, 5), Some(b"ABCBC".to_vec()));
    }
}
