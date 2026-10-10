// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{parsed_named_surface_value, ScalarBodyRefusal, SurfaceNamedValue, SurfacePrototypeFamily};

fn check_steps(
    name: &str,
    body: &[u8],
    steps: &[u64],
    expected: Option<SurfaceNamedValue>,
    limits: (usize, usize, usize),
) {
    let work: u64 = steps.iter().sum();
    for cap in 0..=work + 1 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_retained_bytes = u64_from_index(limits.0);
        policy.limits.max_collection_items = u64_from_index(limits.1);
        policy.limits.max_materialized_bytes = u64_from_index(limits.2);
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || parsed_named_surface_value(&ctx, &SurfacePrototypeFamily::Plane, name, body,
            &ScalarCache::default(), &mut ScalarBodyRefusal::default(), None).transpose();
        let original = if cap < work {
            let CodecError::ResourceLimit(refusal) = run().expect_err("executed dispatch, range or backing move") else { panic!("resource refusal"); };
            let mut used = 0;
            let additional = steps.iter().copied().find(|additional| {
                if used + additional > cap { true } else { used += additional; false }
            }).expect("source-derived charge exceeds cap");
            assert_eq!((refusal.used, refusal.additional), (used, additional));
            refusal
        } else {
            assert_eq!(run().expect("exact execution and storage bounds"), expected);
            let refusal = ctx.charge_work_limit(cap - work + 1, "after named surface value")
                .expect_err("exact remaining Work");
            assert_eq!((refusal.used, refusal.additional), (work, cap - work + 1));
            refusal
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

fn compact_count(count: usize) -> Vec<u8> {
    if count <= 127 { vec![u8::try_from(count).expect("direct count")] }
    else {
        assert_eq!(count, 257);
        vec![0x81, 0x01]
    }
}

// Each emitted value executes one dispatch. Amortized vector growth starts at
// four slots and doubles; growth admits the old capacity in bytes as move Work
// and overlap, and the final capacity accounts for the retained backing.
fn pushed_value_cost(count: usize, item_bytes: usize) -> (Vec<u64>, usize, usize) {
    let mut steps = Vec::new();
    let mut capacity = 0;
    let mut overlap = 0;
    for index in 0..count {
        steps.push(1);
        if index == capacity {
            if capacity != 0 {
                let moved = capacity * item_bytes;
                steps.push(u64_from_index(moved));
                overlap = moved;
            }
            capacity = (capacity * 2).max(4);
        }
    }
    (steps, capacity * item_bytes, overlap)
}

#[test]
fn named_compact_array_dispatch_owns_each_integer_and_skips_absent_slots() {
    for count in [0_usize, 1, 4, 7, 17, 257] {
        let mut body = vec![0xf8];
        body.extend(compact_count(count));
        body.extend(std::iter::repeat_n(7, count));
        let (steps, retained, overlap) = pushed_value_cost(count, std::mem::size_of::<u32>());
        check_steps("dum_array", &body, &steps, Some(SurfaceNamedValue::CompactIntArray(vec![7; count])),
            (retained, count, overlap));
    }
    check_steps("dum_array", &[0xf8, 3, 0x80, 0x80, 7, 8], &[1, 1, 1],
        Some(SurfaceNamedValue::CompactIntArray(vec![128, 7, 8])), (4 * std::mem::size_of::<u32>(), 3, 0));
    // Two encoded integers and the existing scalar fallback dispatch. The
    // absent third and fourth integers cause no visit or allocation.
    check_steps("dum_array", &[0xf8, 4, 7, 8], &[1, 1, 1], None,
        (4 * std::mem::size_of::<u32>(), 2, 0));
    check_steps("dum_array", &[0xf8, 0x81, 1], &[1], None, (0, 0, 0));
}

#[test]
fn named_contiguous_references_admit_exact_generated_range_before_writes() {
    for count in [0_usize, 1, 4, 7, 17, 257] {
        let mut body = vec![0xf8];
        body.extend(compact_count(count));
        body.extend_from_slice(&[0xf7, 0x80, 0x80, 0xfb]);
        let steps = if count == 0 { Vec::new() } else { vec![u64_from_index(count)] };
        let capacity = if count == 0 { 0 } else { count.max(4) };
        let end = 128 + u32::try_from(count).expect("small count");
        check_steps("c_pnts", &body, &steps,
            Some(SurfaceNamedValue::ContiguousEntityReferences((128..end).collect())),
            (capacity * std::mem::size_of::<u32>(), count, 0));
    }
}

#[test]
fn named_scalar_sequence_dispatch_stops_at_first_boundary_and_keeps_interiors() {
    for count in [0_usize, 1, 4, 7, 17, 257] {
        let body = vec![0xe4; count];
        let (mut steps, retained, overlap) = pushed_value_cost(count, std::mem::size_of::<f64>());
        let expected = if count == 0 { SurfaceNamedValue::Empty }
            else { SurfaceNamedValue::ScalarSequence(vec![1.0; count]) };
        check_steps("data_dbls", &body, &steps, Some(expected), (retained, count, overlap));
        let mut bounded = body;
        bounded.extend(std::iter::repeat_n(0xe3, 257));
        steps.push(1); // The first actual boundary, with no visit to its tail.
        let expected = (count != 0).then(|| SurfaceNamedValue::ScalarSequence(vec![1.0; count]));
        check_steps("data_dbls", &bounded, &steps, expected, (retained, count, overlap));
    }
    let raw = [0x46, 0x00, 0xe3, 0xe0, 0xf7, 0xe4, 0x0f, 0x18];
    let value = f64::from_bits(0x4000_e3e0_f7e4_0f18);
    check_steps("data_dbls", &raw, &[1], Some(SurfaceNamedValue::ScalarSequence(vec![value])),
        (4 * std::mem::size_of::<f64>(), 1, 0));
    check_steps("radius", &[0x0d, 0x0e], &[1, 1], Some(SurfaceNamedValue::ScalarSequence(vec![0.25, 0.5])),
        (4 * std::mem::size_of::<f64>(), 2, 0));
}

#[test]
fn named_value_empty_and_fixed_metadata_routes_preserve_original_refusal() {
    for (name, body, expected) in [
        ("data_dbls", &[][..], Some(SurfaceNamedValue::Empty)),
        ("id", &[7][..], Some(SurfaceNamedValue::CompactInt(7))),
        ("flip", &[0xf1, 7][..], Some(SurfaceNamedValue::CompactInt(7))),
        ("flip", &[0xf1][..], None),
        ("offset_type", &[1, 0xf1, 0xf7, 2][..], Some(SurfaceNamedValue::CompactInt(1))),
        ("offset_type", &[1, 0xf1, 0xf7][..], None),
        ("data_dbls", &[0xf9][..], None),
    ] {
        check_steps(name, body, &[], expected, (0, 0, 0));
    }
}
