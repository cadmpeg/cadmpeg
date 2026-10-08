use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ContainerSummary;

use crate::{CodecBackend, CreoCodec};

const SCALAR_LABEL: &[u8] = b"double_xar\0";
const SCALAR_IMAGE_COUNT: usize = 256;
const EXPANDED_PAYLOAD_LEN: usize = SCALAR_LABEL.len() + 1 + 2 + SCALAR_IMAGE_COUNT * 8 + 1;

fn scalar_payload(image_count: usize) -> Vec<u8> {
    let slot_count = image_count + 1;
    let mut payload = Vec::with_capacity(EXPANDED_PAYLOAD_LEN);
    payload.extend_from_slice(SCALAR_LABEL);
    payload.push(0xf8);
    if slot_count <= 0x7f {
        payload.push(u8::try_from(slot_count).expect("short count fits one byte"));
    } else {
        payload.push(0x80 | u8::try_from(slot_count >> 8).expect("two-byte count high part fits"));
        payload.push(u8::try_from(slot_count & 0xff).expect("two-byte count low part fits"));
    }
    for image in 0..image_count {
        payload.extend_from_slice(&[
            0x46,
            u8::try_from(image).expect("the fixture has at most 256 images"),
            0,
            0,
            0,
            0,
            0,
            0,
        ]);
    }
    payload.push(0xe0);
    payload.resize(EXPANDED_PAYLOAD_LEN, 0);
    payload
}

fn scalar_section(image_count: usize) -> Vec<u8> {
    let payload = scalar_payload(image_count);
    let expanded_length = payload.len();
    let compressed = unix_compress_literals(&payload);
    crate::test_support::build_toc_section_prt("ScalarData", &compressed, expanded_length)
}

fn unix_compress_literals(payload: &[u8]) -> Vec<u8> {
    const MAX_BITS: usize = 16;
    const DICTIONARY_LIMIT: usize = 1 << MAX_BITS;

    let mut stream = vec![0x1f, 0x9d, u8::try_from(MAX_BITS).expect("max bits fit u8")];
    let mut widths = Vec::with_capacity(payload.len());
    let mut width = 9;
    let mut free_entry = 256;
    for index in 0..payload.len() {
        if index > 0 && free_entry > (1 << width) - 1 && width < MAX_BITS {
            width += 1;
        }
        widths.push(width);
        if index > 0 && free_entry < DICTIONARY_LIMIT {
            free_entry += 1;
        }
    }

    let mut start = 0;
    while start < payload.len() {
        let code_width = widths[start];
        let mut end = start + 1;
        while end < payload.len() && widths[end] == code_width {
            end += 1;
        }
        for codes in payload[start..end].chunks(8) {
            let byte_count = codes.len().saturating_mul(code_width).div_ceil(8);
            let mut packed = vec![0; byte_count];
            for (index, value) in codes.iter().copied().enumerate() {
                for bit in 0..code_width {
                    let offset = index * code_width + bit;
                    let bit_value = usize::from(value) >> bit & 1;
                    packed[offset / 8] |=
                        u8::try_from(bit_value).expect("one bit fits u8") << (offset % 8);
                }
            }
            if codes.len() < 8 && end < payload.len() {
                // The decoder reads one full width-sized block before it
                // changes code width, even when this block has few codes.
                packed.resize(code_width, 0);
            }
            stream.extend_from_slice(&packed);
        }
        start = end;
    }
    stream
}

fn inspect_with_retained_limit(
    input: &[u8],
    max_retained_bytes: u64,
) -> Result<ContainerSummary, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = max_retained_bytes;
    let (ctx, root) = DecodeContext::from_root_bytes(input, &arena, &policy)?;
    CreoCodec.inspect_impl(&ctx, root)
}

fn minimum_retained_limit(input: &[u8]) -> u64 {
    crate::test_support::allocation_limit_at(
        ResourceDimension::RetainedBytes,
        None,
        |limit| inspect_with_retained_limit(input, limit),
    )
}

#[test]
fn inspection_scan_scratch_does_not_raise_retained_threshold() {
    let short = scalar_section(1);
    let many = scalar_section(SCALAR_IMAGE_COUNT);
    let service_retained_limit = DecodePolicy::service().limits.max_retained_bytes;
    let short_required = minimum_retained_limit(&short);
    let many_required = minimum_retained_limit(&many);

    assert!(short_required < service_retained_limit);
    assert!(many_required < service_retained_limit);
    assert_eq!(short_required, many_required);
    assert_eq!(
        inspect_with_retained_limit(&short, short_required).expect("short inspection"),
        inspect_with_retained_limit(&many, many_required).expect("many-image inspection")
    );
}
