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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
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
    let mut dictionary_storage = ctx.reserve_scoped(0, "decode Creo LZW dictionary")?;
    let mut prefix = dictionary_storage
        .with_storage(|| ctx.alloc_filled(dictionary_limit, 0u16, "creo LZW prefix slots"))?;
    let mut suffix = dictionary_storage
        .with_storage(|| ctx.alloc_filled(dictionary_limit, 0u8, "creo LZW suffix slots"))?;
    for (value, slot) in suffix.iter_mut().take(256).enumerate() {
        let Ok(value) = u8::try_from(value) else {
            return Ok(None);
        };
        *slot = value;
    }

    let mut reader = CodeReader::new(rest, max_bits);
    let mut free_entry = if block_mode { 257usize } else { 256usize };
    let Some(first) = reader.next_in_lane(ctx, free_entry, false)? else {
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
    let mut written: usize = 1;
    let mut stack_storage = ctx.reserve_scoped(0, "Creo LZW stack storage")?;
    let mut stack = stack_storage
        .with_storage(|| ctx.collection_vec(dictionary_limit, "creo LZW stack slots"))?;

    while let Some(raw_code) = reader.next_in_lane(ctx, free_entry, false)? {
        if block_mode && raw_code == CLEAR {
            free_entry = 257;
            let Some(code) = reader.next_in_lane(ctx, free_entry, true)? else {
                return Err(CodecError::malformed(
                    "Creo LZW clear code has no following literal",
                ));
            };
            let code = usize::from(code);
            if code >= 256 {
                return Err(CodecError::malformed(
                    "Creo LZW clear code is followed by a nonliteral",
                ));
            }
            old_code = code;
            let Ok(byte) = u8::try_from(code) else {
                return Ok(None);
            };
            final_byte = byte;
            written = written.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "creo LZW expansion length",
                    u64_from_index(expected_length),
                    u64::MAX,
                )
            })?;
            if written > expected_length {
                return Err(CodecError::malformed(
                    "Creo LZW expansion exceeds its TOC length",
                ));
            }
            output.write(&[final_byte])?;
            continue;
        }

        let input_code = usize::from(raw_code);
        let mut code = input_code;
        if code >= free_entry {
            if code != free_entry {
                return Err(CodecError::malformed(
                    "Creo LZW stream contains an undefined code",
                ));
            }
            if stack.len() == dictionary_limit {
                return Ok(None);
            }
            stack.push(final_byte);
            code = old_code;
        }
        let mut chain_steps = 0..dictionary_limit;
        while code >= 256 {
            if ctx
                .next_charged(&mut chain_steps, "decode Creo LZW dictionary chain")?
                .is_none()
            {
                return Ok(None);
            }
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
            return Err(CodecError::malformed(
                "Creo LZW expansion exceeds its TOC length",
            ));
        }
        output.write(&[final_byte])?;
        if stack.len() > 1 {
            ctx.reverse(stack.as_mut_slice(), "creo LZW stack reversal")?;
        }
        output.write(&stack)?;
        stack.clear();
        written = next_written;

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

    if written != expected_length {
        return Err(CodecError::malformed(
            "Creo LZW expansion does not equal its TOC length",
        ));
    }
    if !reader.padding_is_zero() {
        return Err(CodecError::malformed(
            "Creo LZW stream has invalid final padding",
        ));
    }
    finish_expansion(output, reservation).map(Some)
}

fn finish_expansion(
    output: ExpandWriter<'_, '_>,
    reservation: ScopedReservation<'_>,
) -> Result<Vec<u8>, CodecError> {
    let bytes = output.finalize_owned()?;
    reservation.commit_value(bytes)
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

    fn padding_is_zero(&self) -> bool {
        if self.cursor != self.data.len() {
            return false;
        }
        let bits = self.block.len() * 8;
        if self.bit_offset > bits || bits - self.bit_offset >= self.width {
            return false;
        }
        (self.bit_offset..bits).all(|bit| ((self.block[bit / 8] >> (bit % 8)) & 1) == 0)
    }

    fn next_in_lane(
        &mut self,
        ctx: &DecodeContext<'_>,
        free_entry: usize,
        clear: bool,
    ) -> Result<Option<u16>, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        if !self.has_code(free_entry, clear) {
            // The terminal transition still establishes the final block and
            // padding state. Its width/cardinality checks have fixed size.
            return Ok(self.next(free_entry, clear));
        }
        ctx.next_charged(
            &mut std::iter::from_fn(|| self.next(free_entry, clear)),
            "decode Creo LZW code",
        )
    }

    fn has_code(&self, free_entry: usize, clear: bool) -> bool {
        // The private width starts at nine and grows only up to max_bits,
        // whose header grammar admits nine through sixteen.
        let grows = free_entry > (1usize << self.width) - 1 && self.width < self.max_bits;
        let reset = clear || grows;
        let width = if clear { 9 } else { self.width + usize::from(grows) };
        let bit_offset = if reset { 0 } else { self.bit_offset };
        let start_limit = if reset { 0 } else { self.start_limit };
        if bit_offset < start_limit {
            return true;
        }
        let Some(remaining) = self.data.len().checked_sub(self.cursor) else {
            return false;
        };
        // next loads at most width bytes. A fresh block has one code exactly
        // when its bit count reaches the width; no bytes are examined here.
        remaining.min(width) * 8 >= width
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
            let end = self.cursor.checked_add(self.width)?.min(self.data.len());
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
        let mut bytes = vec![0; values.len().checked_mul(9).expect("fixture bit count fits").div_ceil(8)];
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
        match super::decode(&ctx, data, expected_length) {
            Ok(value) => value,
            Err(cadmpeg_core::CodecError::Malformed(_)) => None,
            Err(error) => panic!("test stream resource refusal: {error}"),
        }
    }

    #[test]
    fn lzw_decoder_refuses_before_first_and_clear_following_code() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let mut literal_stream = vec![0x1f, 0x9d, 0x09];
        literal_stream.extend(codes(&[65, 66]));
        let mut clear_stream = vec![0x1f, 0x9d, 0x89];
        let mut first_block = codes(&[65, CLEAR]);
        first_block.resize(9, 0);
        clear_stream.extend(first_block);
        clear_stream.extend(codes(&[66]));
        for (stream, after_clear) in [(literal_stream, false), (clear_stream, true)] {
            let allowed = if after_clear {
                let error = crate::test_support::last_refusal_at(
                    &[], ResourceDimension::WorkUnits, "decode Creo LZW code",
                    |ctx| super::decode(ctx, &stream, 2),
                );
                let cadmpeg_core::CodecError::ResourceLimit(limit) = error else { panic!("code refusal"); };
                limit.limit
            } else {
                crate::test_support::allocation_limit_at(
                    ResourceDimension::WorkUnits, Some("decode Creo LZW code"), |cap| {
                        let arena = DecodeArena::new();
                        let mut policy = DecodePolicy::service();
                        policy.limits.max_work_units = cap;
                        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                        super::decode(&ctx, &stream, 2)
                    },
                )
            };
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowed;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let cadmpeg_core::CodecError::ResourceLimit(original) = super::decode(&ctx, &stream, 2)
                .expect_err("next present code refuses before expansion") else { panic!("expected resource refusal"); };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.operation, "decode Creo LZW code");
            assert_eq!((original.used, original.additional), (allowed, 1));
            assert!(matches!(super::decode(&ctx, &stream, 2),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) if refusal == original));
        }
    }

    #[test]
    fn lzw_literal_decoder_uses_exact_code_copy_and_growth_work() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let mut stream = vec![0x1f, 0x9d, 0x09];
        stream.extend(codes(&[65, 66, 67]));
        let work = crate::test_support::allocation_limit_at(
            ResourceDimension::WorkUnits, None, |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                super::decode(&ctx, &stream, 3)
            },
        );
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::WorkUnits, "expand_write copy",
            |ctx| super::decode(ctx, &stream, 3),
        );
        let cadmpeg_core::CodecError::ResourceLimit(original) = error else { panic!("output copy refusal"); };
        assert_eq!(original.operation, "expand_write copy");
        assert_eq!((original.used, original.additional, original.limit), (work - 1, 1, work - 1));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(super::decode(&ctx, &stream, 3).expect("exact work admits literals"), Some(b"ABC".to_vec()));
        let original = ctx.charge_work_limit(1, "after exact literal decode")
            .expect_err("all actual work was admitted");
        assert_eq!((original.used, original.additional), (work, 1));
    }

    #[test]
    fn lzw_code_visits_admit_present_literals_and_leave_terminal_probe_free() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let data = codes(&[65, 66, 67]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut reader = super::CodeReader::new(&data, 16);
        for code in [65, 66, 67] {
            assert_eq!(reader.next_in_lane(&ctx, 256, false).expect("present code"), Some(code));
        }
        assert_eq!(reader.next_in_lane(&ctx, 256, false).expect("free terminal probe"), None);
        assert!(reader.padding_is_zero());
        let original = ctx.charge_work_limit(1, "after three LZW code visits")
            .expect_err("all three present code visits were admitted");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!((original.used, original.additional), (3, 1));
        assert!(matches!(reader.next_in_lane(&ctx, 256, false),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) if refusal == original));
    }

    #[test]
    fn lzw_code_visit_refusal_precedes_reader_state_mutation() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let data = codes(&[65, 66]);
        let decoded = crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits, &["decode Creo LZW code"], |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let mut reader = super::CodeReader::new(&data, 16);
                let mut decoded = Vec::new();
                loop {
                    let before = (reader.cursor, reader.block, reader.bit_offset, reader.start_limit, reader.width);
                    match reader.next_in_lane(&ctx, 256, false) {
                        Ok(Some(code)) => {
                            assert_eq!(code, [65, 66][decoded.len()]);
                            decoded.push(code);
                        }
                        Ok(None) => {
                            assert_eq!(cap, 2);
                            return Ok(decoded);
                        }
                        Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) => {
                            assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                            assert_eq!(refusal.operation, "decode Creo LZW code");
                            assert_eq!((refusal.used, refusal.additional), (cap, 1));
                            assert_eq!((reader.cursor, reader.block, reader.bit_offset, reader.start_limit, reader.width), before);
                            assert_eq!(ctx.resource_refusal(), Some(refusal));
                            return Err(cadmpeg_core::CodecError::ResourceLimit(refusal));
                        }
                        Err(error) => panic!("code reader error: {error}"),
                    }
                }
            },
        );
        assert_eq!(decoded, [65, 66]);
    }

    #[test]
    fn lzw_code_after_clear_has_its_own_present_visit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

        let mut data = codes(&[65, CLEAR]);
        data.resize(9, 0);
        data.extend(codes(&[66, 67]));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut reader = super::CodeReader::new(&data, 16);
        assert_eq!(reader.next_in_lane(&ctx, 257, false).expect("first literal"), Some(65));
        assert_eq!(reader.next_in_lane(&ctx, 257, false).expect("CLEAR code"), Some(CLEAR));
        assert_eq!(reader.next_in_lane(&ctx, 257, true).expect("literal after CLEAR"), Some(66));
        let error = reader.next_in_lane(&ctx, 257, false).expect_err("fourth present code exceeds cap");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::WorkUnits
                && refusal.operation == "decode Creo LZW code"
                && (refusal.used, refusal.additional) == (3, 1)));
    }

    #[test]
    fn lzw_absent_codes_keep_alignment_and_padding_transitions_free() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        for data in [&[][..], &[0][..], &[0x80][..]] {
            for free_entry in [257, 512] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
                let mut reader = super::CodeReader::new(data, 16);
                assert_eq!(reader.next_in_lane(&ctx, free_entry, false).expect("no complete code"), None);
                assert_eq!(reader.width, if free_entry == 512 { 10 } else { 9 });
                assert_eq!(reader.cursor, data.len());
                assert_eq!(reader.padding_is_zero(), data != [0x80]);
                assert!(ctx.resource_refusal().is_none());
            }
        }
    }

    #[test]
    fn invalid_fixed_lzw_headers_are_free_and_preserve_original_refusal() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        use cadmpeg_core::CodecError;

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let headers = [
            &[][..],
            &[0x1f][..],
            &[0x1f, 0x9d][..],
            &[0xff, 0x9d, 0x10][..],
            &[0x1f, 0x9d, 0x08][..],
            &[0x1f, 0x9d, 0x30][..],
        ];
        for header in headers {
            assert_eq!(super::decode(&ctx, header, 0).expect("fixed header rejection"), None);
        }
        let original = ctx.charge_work_limit(1, "seed LZW header refusal")
            .expect_err("zero work limit");
        for header in headers {
            assert!(matches!(super::decode(&ctx, header, 0),
                Err(CodecError::ResourceLimit(refusal)) if refusal == original));
        }
    }

    #[test]
    fn lzw_owned_output_promotes_into_its_parent_scope() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut parent = ctx.reserve_scoped(0, "LZW output parent").expect("empty parent");
        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        let bytes = parent.with_storage(|| super::decode(&ctx, &stream, 3))
            .expect("output belongs to parent")
            .expect("valid literal stream");
        assert_eq!(bytes, b"ABC");
        drop(bytes);
        drop(parent);
        assert!(ctx.resource_refusal().is_none());
    }

    #[test]
    fn toc_expansion_refuses_before_materializing_output() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            Some("inflate Creo TOC section"),
            |cap| {
                let mut trial = policy;
                trial.limits.max_materialized_bytes = cap;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&stream, &arena, &trial).expect("root");
                super::decode(&ctx, &stream, 3)
            },
        );
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
        policy.limits.max_decompressed_bytes_per_expand = crate::test_support::allocation_limit_at(
            ResourceDimension::DecompressedBytes, Some("begin_expand"), |cap| {
                let arena = DecodeArena::new();
                let mut trial = policy;
                trial.limits.max_decompressed_bytes_per_expand = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &trial).expect("root");
                super::decode(&ctx, &stream, 3)
            },
        );
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
        let total = crate::test_support::allocation_limit_at(
            ResourceDimension::DecompressedBytes, None, |cap| {
                let arena = DecodeArena::new();
                let mut trial = policy;
                trial.limits.max_decompressed_bytes_total = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &trial).expect("root");
                assert_eq!(super::decode(&ctx, &stream, 3)?, Some(b"ABC".to_vec()));
                super::decode(&ctx, &stream, 3)
            },
        );
        policy.limits.max_decompressed_bytes_total = total.checked_sub(1).expect("nonempty expansions");
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
    fn lzw_stack_slots_refuse_at_collection_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        let arena = DecodeArena::new();
        let service = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &service)
            .expect("service profile admits input");
        assert_eq!(
            super::decode(&ctx, &stream, 3).expect("service profile admits LZW stack"),
            Some(b"ABC".to_vec())
        );

        let err = crate::test_support::last_refusal_at(
            &stream,
            ResourceDimension::CollectionItems,
            "creo LZW stack slots",
            |ctx| super::decode(ctx, &stream, 3),
        );
        assert!(matches!(
            err,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "creo LZW stack slots"
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
                let bytes = if width == 10 {
                    (chunk.len() * width).div_ceil(8)
                } else {
                    width
                };
                packed.resize(block_offset + bytes, 0);
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
    #[test]
    fn expansion_owns_one_payload_buffer() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // Dictionary and stack storage are released; only the output is retained.
        policy.limits.max_retained_bytes = 3;
        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&stream, &arena, &policy)
                .expect("root");
        assert_eq!(
            super::decode(&ctx, &stream, 3).expect("one retained buffer"),
            Some(b"ABC".to_vec())
        );
    }
    #[test]
    fn lzw_rejects_output_after_declared_prefix() {
        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x84, 0x0c, 0x01];
        crate::decode::with_test_decode_ctx(|ctx| {
            assert!(matches!(
                super::decode(ctx, &stream, 1),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn lzw_rejects_invalid_code_after_declared_prefix() {
        let mut stream = vec![0x1f, 0x9d, 0x10];
        stream.extend(codes(&[65, 400]));
        crate::decode::with_test_decode_ctx(|ctx| {
            assert!(matches!(
                super::decode(ctx, &stream, 1),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }

    #[test]
    fn lzw_rejects_nonzero_terminal_padding() {
        let stream = [0x1f, 0x9d, 0x10, 0x41, 0x80];
        crate::decode::with_test_decode_ctx(|ctx| {
            assert!(matches!(
                super::decode(ctx, &stream, 1),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }
    #[test]
    fn lzw_rejects_invalid_clear_literal_after_declared_prefix() {
        let mut stream = vec![0x1f, 0x9d, 0x90];
        let mut first_block = codes(&[u16::from(b'A'), CLEAR]);
        first_block.resize(9, 0);
        stream.extend(first_block);
        stream.extend(codes(&[CLEAR]));
        crate::decode::with_test_decode_ctx(|ctx| {
            assert!(matches!(
                super::decode(ctx, &stream, 1),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
        });
    }
    #[test]
    fn lzw_stack_reversal_refuses_work() {
        let mut stream = vec![0x1f, 0x9d, 0x10];
        stream.extend(codes(&[65, 66, 256, 258]));
        assert_eq!(decode(&stream, 7), Some(b"ABABABA".to_vec()));
        let error = crate::test_support::last_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "creo LZW stack reversal",
            |ctx| super::decode(ctx, &stream, 7),
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && resource.operation == "creo LZW stack reversal")
        );
    }

    #[test]
    fn lzw_singleton_stack_has_no_reversal_work() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        let mut stream = vec![0x1f, 0x9d, 0x09];
        stream.extend(codes(&[65, 66, 256]));
        let work = crate::test_support::allocation_limit_at(
            ResourceDimension::WorkUnits, None, |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = super::decode(&ctx, &stream, 4);
                if let Err(CodecError::ResourceLimit(limit)) = &result {
                    assert_ne!(limit.operation, "creo LZW stack reversal");
                }
                result
            },
        );
        let error = crate::test_support::last_refusal_at(
            &[], ResourceDimension::WorkUnits, "expand_write copy",
            |ctx| super::decode(ctx, &stream, 4),
        );
        let CodecError::ResourceLimit(original) = error else { panic!("last copied byte exceeds cap"); };
        assert_eq!(original.operation, "expand_write copy");
        assert_eq!((original.dimension, original.used, original.additional, original.limit),
            (ResourceDimension::WorkUnits, work - 1, 1, work - 1));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(super::decode(&ctx, &stream, 4).expect("actual code, chain, copy and growth work"), Some(b"ABAB".to_vec()));
        let original = ctx.charge_work_limit(1, "after singleton-stack decode").expect_err("exact work cap used");
        assert_eq!((original.dimension, original.used, original.additional),
            (ResourceDimension::WorkUnits, work, 1));
        assert!(matches!(super::decode(&ctx, &stream, 4),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}
