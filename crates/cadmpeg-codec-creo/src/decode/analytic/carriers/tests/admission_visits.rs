// SPDX-License-Identifier: Apache-2.0
use super::super::{
    ordered_contained_face_loops, ordered_parameter_face_loops, polygon_strictly_contains,
    polygon_strictly_contains_polygon, positional_cylinder_carrier,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn visit_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    policy
}

#[test]
fn carrier_fixed_preconditions_preserve_original_refusal_without_work() {
    let scan = crate::test_support::empty_container_scan();
    let source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let plane_row = super::carrier_row(17, crate::surface::SurfaceKind::Plane);
    let cylinder_row = super::carrier_row(17, crate::surface::SurfaceKind::Cylinder);
    let lp = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        (10..13).map(|curve_id| crate::topology::HalfEdgeId {
            curve_id, side: crate::topology::Side::Zero,
        }).collect(),
    );
    let arena = DecodeArena::new();
    let policy = visit_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let check = |refused| {
        let results = [
            positional_cylinder_carrier(&ctx, &scan, &plane_row, &scan.surfaces.parameters, None, &source_carriers).map(|carrier| carrier.is_none()),
            positional_cylinder_carrier(&ctx, &scan, &cylinder_row, &scan.surfaces.parameters, None, &source_carriers).map(|carrier| carrier.is_none()),
            polygon_strictly_contains(&ctx, &[], [1.0, 1.0]).map(|contains| !contains),
            polygon_strictly_contains(&ctx, &[[0.0, 0.0], [2.0, 0.0]], [1.0, 1.0]).map(|contains| !contains),
            polygon_strictly_contains_polygon(&ctx, &[], &[]),
            ordered_contained_face_loops(&ctx, Vec::new(), &[]).map(|ordered| ordered.is_none()),
            ordered_contained_face_loops(&ctx, vec![&lp], &[]).map(|ordered| ordered.is_none()),
            ordered_contained_face_loops(&ctx, vec![&lp, &lp], &[]).map(|ordered| ordered.is_none()),
            ordered_parameter_face_loops(&ctx, vec![&lp], &[]).map(|ordered| {
                let ordered = ordered.expect("one supplied loop");
                ordered.len() == 1 && std::ptr::eq(ordered[0], &lp)
            }),
        ];
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert!(result.expect("fixed precondition"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after carrier fixed preconditions").expect_err("zero cap");
    assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, 0, 1));
    check(true);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

fn assert_visits(
    operations: &[&'static str],
    expected: bool,
    query: impl Fn(&DecodeContext<'_>) -> Result<bool, CodecError>,
) {
    let visits = operations.len() as u64;
    for cap in 0..=visits {
        let arena = DecodeArena::new();
        let policy = visit_policy(cap);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = query(&ctx);
        let original = if cap == visits {
            assert_eq!(result.expect("present comparisons"), expected);
            let original = ctx.charge_work_limit(1, "after carrier comparisons").expect_err("exact visits");
            assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, visits, 1));
            original
        } else {
            let original = ctx.resource_refusal().expect("comparison refusal");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.limit, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, cap, cap, 1, operations[cap as usize]));
            original
        };
        assert!(matches!(query(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn polygon_containment_visits_present_edges_and_stops_at_contact() {
    const EDGE: &str = "creo polygon containment points";
    let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
    for (point, visits, expected) in [
        ([2.0, 2.0], 4, true), ([-1.0, 2.0], 4, false),
        ([2.0, 0.0], 1, false), ([2.0, 4.0], 3, false),
    ] {
        assert_visits(&[EDGE; 4][..visits], expected, |ctx| polygon_strictly_contains(ctx, &square, point));
    }
    let mut first_contact = square.to_vec();
    first_contact.extend(std::iter::repeat_n([0.0, 4.0], 128));
    assert_visits(&[EDGE], false, |ctx| polygon_strictly_contains(ctx, &first_contact, [2.0, 0.0]));
}

#[test]
fn polygon_pair_containment_admits_each_executed_vertex_and_edge_visit() {
    const VERTEX: &str = "creo contained polygon vertices";
    const POINT: &str = "creo polygon containment points";
    const INNER_EDGE: &str = "creo contained polygon edges";
    const OUTER_EDGE: &str = "creo containing polygon edges";
    let outer = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
    let inner = [[1.0, 1.0], [2.0, 1.0], [1.0, 2.0]];
    // Three vertices each visit one inner point and all four containing edges;
    // three inner edges each compare with all four containing edges: 15 + 15.
    let mut operations = Vec::new();
    for _ in 0..3 {
        operations.push(VERTEX);
        operations.extend([POINT; 4]);
    }
    for _ in 0..3 {
        operations.push(INNER_EDGE);
        operations.extend([OUTER_EDGE; 4]);
    }
    assert_visits(&operations, true, |ctx| polygon_strictly_contains_polygon(ctx, &outer, &inner));
    assert_visits(&[], true, |ctx| polygon_strictly_contains_polygon(ctx, &outer, &[]));
    let withheld = vec![[1.0, 1.0]; 129];
    // A two-point outer fails its fixed precondition at the first inner point.
    assert_visits(&[VERTEX], false, |ctx| polygon_strictly_contains_polygon(ctx, &outer[..2], &withheld));
    let outside = vec![[-1.0, 2.0]; 129];
    assert_visits(&[VERTEX, POINT, POINT, POINT, POINT], false, |ctx| polygon_strictly_contains_polygon(ctx, &outer, &outside));
}
