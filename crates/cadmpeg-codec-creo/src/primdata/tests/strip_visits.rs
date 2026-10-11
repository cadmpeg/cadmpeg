// SPDX-License-Identifier: Apache-2.0
use super::super::triangle_strips;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

pub(super) fn strip_input(offset: usize, spans: &[u32], vertices: usize, shaded: bool) -> Vec<u8> {
    let positions = vec![[0.0; 3]; vertices];
    let normals = shaded.then(|| vec![[0.0, 1.0, 0.0]; vertices]);
    strip_geometry_input(offset, spans, &positions, normals.as_deref())
}

pub(super) fn strip_geometry_input(
    offset: usize, spans: &[u32], positions: &[[f64; 3]], normals: Option<&[[f64; 3]]>,
) -> Vec<u8> {
    let mut bytes = vec![0xff; offset];
    bytes.extend_from_slice(b"value(prim_tristripsetwithatt)\0\xe0\x01p_accum_set_size\0\xf8");
    bytes.push(u8::try_from(spans.len()).expect("small fixture table"));
    let mut cumulative = 0;
    for span in spans {
        cumulative += span;
        bytes.push(u8::try_from(cumulative).expect("small cumulative count"));
    }
    let mut values = Vec::new();
    let mut scalar_count = 0;
    for (index, position) in positions.iter().enumerate() {
        let normal = normals.map(|normals| normals[index]);
        for vector in normal.iter().chain(std::iter::once(position)) {
            for value in vector {
                match *value {
                    0.0 => values.push(0),
                    1.0 => values.extend_from_slice(&[0x46, 0x80, 0, 0]),
                    _ => panic!("fixture uses only zero and unit coordinates"),
                }
                scalar_count += 1;
            }
        }
    }
    let field = if normals.is_some() { "mv_p_NxNyNzxyz" } else { "mv_p_xyz" };
    bytes.extend(super::named(field, &values, u8::try_from(scalar_count).expect("small scalar count")));
    bytes
}

#[test]
fn empty_strip_validation_is_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let scan = triangle_strips(&ctx, &[]).expect("no span");
    assert!(scan.strips.is_empty());
    assert_eq!(scan.conflicting_representation_count, 0);
}

#[test]
fn strip_validation_admits_present_spans_and_stops_at_the_first_invalid_span() {
    for (spans, vertices, valid) in [([2, 3], 3, false), ([3, 2], 6, false), ([2, 3], 5, false), ([3, 2], 5, false), ([3, 3], 6, true)] {
        let bytes = strip_input(17, &spans, vertices, false);
        let result = super::work_output(|ctx| triangle_strips(ctx, &bytes));
        assert_eq!(result.conflicting_representation_count, 0);
        if valid {
            let [strip] = result.strips.as_slice() else { panic!("complete source strip"); };
            assert_eq!(strip.offset, 17);
            assert_eq!(strip.strip_lengths(), &spans);
            assert_eq!(strip.positions().len(), vertices);
            assert!(strip.positions().all(|position| position.get() == [0.0; 3]));
            assert!(strip.normals().is_none());
        } else {
            assert!(result.strips.is_empty());
        }
    }
}

#[test]
fn primitive_parser_rejects_short_span_with_nonzero_positions() {
    let positions = [[1.0, 0.0, 0.0]; 3];
    let invalid = strip_geometry_input(0, &[2], &positions, None);
    let rejected = super::work_output(|ctx| triangle_strips(ctx, &invalid));
    assert!(rejected.strips.is_empty());
    assert_eq!(rejected.conflicting_representation_count, 0);
    let valid = strip_geometry_input(0, &[3], &positions, None);
    let admitted = super::work_output(|ctx| triangle_strips(ctx, &valid));
    let [strip] = admitted.strips.as_slice() else { panic!("complete three-vertex strip"); };
    assert_eq!(strip.positions().map(|point| point.get()).collect::<Vec<_>>(), positions);
}

#[test]
fn short_strip_spans_stop_before_geometry_parsing_at_exact_work() {
    use cadmpeg_core::decode::ResourceDimension;
    const RECORD: &[u8] = b"value(prim_tristripsetwithatt)\0";
    const BOUNDARY: &[u8] = b"\xe0\x00value(";
    for (spans, length_visits) in [([2, 3], 1), ([3, 2], 2)] {
        let bytes = strip_input(17, &spans, 5, false);
        let tail = bytes.len() - 17 - RECORD.len();
        let expected = bytes.len() - RECORD.len() + 1 // Global discovery.
            + tail - BOUNDARY.len() + 2 // Boundary windows and end probe.
            + RECORD.len() + 1 // Cumulative label.
            + spans.len() + length_visits; // Declared counts and prefix before invalid span.
        let work = crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 4;
            policy.limits.max_materialized_bytes = u64::try_from(4 * std::mem::size_of::<u32>()).expect("table backing");
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            triangle_strips(&ctx, &bytes)
        });
        assert_eq!(work, u64::try_from(expected).expect("fixture work"));
    }
}
