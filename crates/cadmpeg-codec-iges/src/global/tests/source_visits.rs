// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::mem::size_of;
use std::ops::Range;

fn long_hollerith() -> Vec<u8> {
    let mut bytes = b",,100H".to_vec();
    bytes.extend(std::iter::repeat_n(b'a', 100));
    bytes.push(b';');
    bytes
}

#[test]
fn global_field_copy_source_refuses_one_byte_after_actual_layout_prefix() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Three field-discovery visits, one moved Range slot, then the field
    // span, leading-space and non-Hollerith visits precede the first copy.
    let prefix = 3 + size_of::<Range<usize>>() + 3;
    policy.limits.max_work_units = u64::try_from(prefix).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = super::super::layout_global_cards(b",,;", &ctx) else {
        panic!("expected one global field-copy source refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges global layout field copy");
    assert_eq!((first.limit, first.used, first.additional), (policy.limits.max_work_units,
        policy.limits.max_work_units, 1));
    for bytes in [b",,;".as_slice(), &[]] {
        assert!(matches!(super::super::layout_global_cards(bytes, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn global_field_copy_allocation_refuses_before_uncopied_hollerith_tail() {
    let bytes = long_hollerith();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The two field slots are admitted before copying. The first 72-byte
    // card holds two delimiter bytes and 70 bytes of the Hollerith field.
    // Its descriptor is allocated after the next source byte is visited.
    let before_long_copy = 12 + size_of::<Range<usize>>() + 5 + 12;
    policy.limits.max_work_units = u64::try_from(before_long_copy + 71).unwrap();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = super::super::layout_global_cards(&bytes, &ctx) else {
        panic!("expected first global card descriptor refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges global layout cards");
    assert_eq!((first.limit, first.used, first.additional), (2, 2, 1));
    assert!(matches!(super::super::layout_global_cards(&bytes, &ctx),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn global_field_copy_preserves_short_stream_with_exact_current_bounds() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Three discovery visits, one Range-slot move, five visits for the
    // first field, four for the second, and the terminal field-span probe.
    policy.limits.max_work_units = u64::try_from(3 + size_of::<Range<usize>>() + 5 + 4 + 1).unwrap();
    // Range capacity grows from one to four, with the original slot live
    // during growth. Output owns one 72-byte card and four descriptor slots.
    policy.limits.max_materialized_bytes = u64::try_from(5 * size_of::<Range<usize>>()).unwrap();
    policy.limits.max_retained_bytes = u64::try_from(72 + 4 * size_of::<Vec<u8>>()).unwrap();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let cards = super::super::layout_global_cards(b",,;", &ctx).unwrap();
    assert_eq!(cards, [b",,;".to_vec()]);
    drop(cards);
    let released = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
        "test released global field spans").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

#[test]
fn global_field_copy_preserves_hollerith_crossing_with_exact_current_bounds() {
    let bytes = long_hollerith();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Discovery 12, the Range-slot move, first-field layout/copy 5,
    // second-field header visits 12, its 105 copied bytes, and terminal 1.
    policy.limits.max_work_units = u64::try_from(12 + size_of::<Range<usize>>() + 5 + 12 + 105 + 1).unwrap();
    policy.limits.max_materialized_bytes = u64::try_from(5 * size_of::<Range<usize>>()).unwrap();
    policy.limits.max_retained_bytes = u64::try_from(2 * 72 + 4 * size_of::<Vec<u8>>()).unwrap();
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let cards = super::super::layout_global_cards(&bytes, &ctx).unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0], bytes[..72]);
    assert_eq!(cards[1], bytes[72..]);
    drop(cards);
    let released = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
        "test released global Hollerith spans").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}
