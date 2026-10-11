// SPDX-License-Identifier: Apache-2.0
//! Offset candidate indexing and admission.

use super::super::{offset_support_carriers, B2OffsetSupport};
use crate::families::a5a8::records::FreeformSurface;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::IncreasingParameterInterval;

fn surface(u: [f64; 2], v: Vec<f64>) -> FreeformSurface {
    let count = v.len() - 2;
    let poles = vec![vec![Point3::new(0.0, 0.0, 0.0); count]; 2];
    let geometry = crate::test_support::with_service_context(|ctx| {
        NurbsSurface::new(
            ctx,
            NurbsSurfaceAxis::new(1, vec![u[0], u[0], u[1], u[1]], false),
            NurbsSurfaceAxis::new(1, v, false),
            NurbsPoleGrid::Polynomial { rows: poles },
            false,
        )
    })
    .expect("admitted lanes")
    .expect("surface");
    FreeformSurface {
        pos: 0,
        identity: None,
        geometry,
    }
}

fn offset(u: [f64; 2], v: [f64; 2]) -> B2OffsetSupport {
    B2OffsetSupport {
        pos: 0,
        support_id: 1,
        distance: FiniteReal::new(0.0).expect("finite"),
        u_range: IncreasingParameterInterval::new(u).expect("u interval"),
        v_range: IncreasingParameterInterval::new(v).expect("v interval"),
    }
}

#[test]
fn offset_knot_search_preserves_rounded_absolute_difference() {
    let carrier = surface(
        [0.0, 1.0],
        vec![
            0.0,
            0.0,
            3.969_197_063_200_492_7,
            100.0,
            30_639.389_180_574_413,
            30_639.389_180_574_413,
        ],
    );
    let query = offset([0.0, 1.0], [34.608_586_243_774_91, 100.0]);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| offset_support_carriers(
            ctx,
            std::slice::from_ref(&query),
            std::slice::from_ref(&carrier)
        ))
        .expect("direct search"),
        [Some(0)]
    );
    let carriers = [carrier, surface([2.0, 3.0], vec![0.0, 0.0, 1.0, 1.0])];
    assert_eq!(
        crate::test_support::with_retained_limit(
            2 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Option<usize>>()),
            |ctx| offset_support_carriers(ctx, &[query.clone(), query], &carriers)
        )
        .expect("only binding slots retained"),
        [Some(0), Some(0)]
    );
}

#[test]
fn offset_ambiguity_does_not_visit_the_unread_tail() {
    let mut carriers = vec![surface([0.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]); 2];
    carriers.extend((0..64).map(|_| surface([2.0, 3.0], vec![0.0, 0.0, 1.0, 1.0])));
    // One offset visit and two carrier visits, each with three and two knot
    // partition steps, use thirteen units. Ambiguity stops before the tail.
    assert_eq!(
        crate::test_support::with_work_limit(13, |ctx| offset_support_carriers(
            ctx,
            &[offset([0.0, 1.0], [0.0, 1.0])],
            &carriers
        ))
        .expect("first two matches decide"),
        [None]
    );
}

#[test]
fn ordered_offset_domains_select_each_disjoint_carrier() {
    let carriers: Vec<_> = (0..16_384)
        .map(|index| {
            let lower = f64::from(index) * 2.0;
            surface([lower, lower + 1.0], vec![0.0, 0.0, 1.0, 1.0])
        })
        .collect();
    let queries: Vec<_> = (0..16_384)
        .rev()
        .map(|index| {
            let lower = f64::from(index) * 2.0;
            offset([lower, lower + 1.0], [0.0, 1.0])
        })
        .collect();
    // A full carrier scan for every query needs at least 16_384*16_384 visits.
    let result = crate::test_support::with_work_limit(16_384 * 16_384 - 1, |ctx| {
        offset_support_carriers(ctx, &queries, &carriers)
    })
    .expect("ordered domain walks");
    assert_eq!(result, (0..16_384).rev().map(Some).collect::<Vec<_>>());
}

#[test]
fn no_offsets_build_no_candidate_index() {
    let carriers = [surface([0.0, 1.0], vec![0.0, 0.0, 1.0, 1.0])];
    assert_eq!(
        crate::test_support::with_work_limit(0, |ctx| offset_support_carriers(ctx, &[], &carriers))
            .expect("no consumer work"),
        []
    );
}

#[test]
fn two_offset_queries_do_not_materialize_long_unused_knot_lanes() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let knots: Vec<_> = std::iter::once(0.0)
        .chain((0..65_536).map(|index| f64::from(index) / 65_535.0))
        .chain(std::iter::once(1.0))
        .collect();
    let carriers = [
        surface([0.0, 1.0], knots.clone()),
        surface([2.0, 3.0], knots),
    ];
    let queries = [
        offset([0.0, 1.0], [0.0, 1.0]),
        offset([2.0, 3.0], [0.0, 1.0]),
    ];
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "catia_b2_offset_knot_index_scan",
        None,
    );
    assert_eq!(
        crate::test_support::with_work_limit(u64::MAX, |ctx| offset_support_carriers(
            ctx, &queries, &carriers
        ))
        .expect("domain candidates need only binary knot searches"),
        [Some(0), Some(1)]
    );
}

#[test]
fn offset_active_slots_reuse_backing_while_one_carrier_stays_live() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let mut carriers = vec![surface([0.0, 256.0], vec![0.0, 0.0, 1.0, 1.0])];
    carriers.extend((0..128).map(|index| {
        let lower = f64::from(index) * 2.0;
        surface([lower, lower + 1.0], vec![0.0, 0.0, 1.0, 1.0])
    }));
    let queries: Vec<_> = (0..128)
        .map(|index| {
            let lower = f64::from(index) * 2.0;
            offset([lower, lower + 1.0], [0.0, 1.0])
        })
        .collect();
    // An ordered usize set node used 11 key slots, 16 pointer-width slots,
    // and two alignment slots. The active vector now holds at most two ids
    // in reusable backing; no such node is allocated on any entry cycle.
    let node_bytes = cadmpeg_core::decode::u64_from_index(29 * std::mem::size_of::<usize>());
    let _probe = RefusalProbe::arm(
        ResourceDimension::MaterializedBytes,
        "catia_b2_offset_active_carriers",
        Some(node_bytes),
    );
    assert_eq!(
        crate::test_support::with_materialized_limit(u64::MAX, |ctx| {
            offset_support_carriers(ctx, &queries, &carriers)
        })
        .expect("reusable active backing"),
        vec![None; 128]
    );
}

#[test]
fn invalid_u_spans_do_not_populate_optional_knot_indexes() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let knots: Vec<_> = std::iter::once(0.0)
        .chain((0..1024).map(|index| f64::from(index) / 1023.0))
        .chain(std::iter::once(1.0))
        .collect();
    let carriers = [
        surface([0.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]),
        surface([0.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]),
        surface([0.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]),
        surface([-f64::MAX, f64::MAX], knots),
    ];
    let queries = vec![offset([0.0, 1.0], [0.0, 1.0]); 8193];
    // 8193*3 domain candidates exceed 12*2048, so the valid knot bands are
    // built. The overflowing U span cannot bind and must not visit 1026 knots.
    let refused =
        crate::test_support::with_work_refusal("catia_b2_offset_knot_index_scan", |ctx| {
            offset_support_carriers(ctx, &queries, &carriers)
        });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_b2_offset_knot_index_scan")
    );
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "catia_b2_offset_knot_index_scan",
        Some(1026),
    );
    let bindings = crate::test_support::with_work_limit(u64::MAX, |ctx| {
        offset_support_carriers(ctx, &queries, &carriers)
    })
    .expect("ineligible carriers have no knot index");
    assert_eq!(bindings, vec![None; 8193]);
}

#[test]
fn offsets_without_eligible_domains_build_no_query_or_active_index() {
    let carriers = [
        surface([-f64::MAX, f64::MAX], vec![0.0, 0.0, 1.0, 1.0]),
        surface([0.0, 1.0], vec![-f64::MAX, -f64::MAX, f64::MAX, f64::MAX]),
    ];
    let queries = [
        offset([0.0, 1.0], [0.0, 1.0]),
        offset([0.0, 1.0], [0.0, 1.0]),
    ];
    assert_eq!(
        crate::test_support::with_materialized_limit(0, |ctx| {
            offset_support_carriers(ctx, &queries, &carriers)
        })
        .expect("only unresolved binding slots survive the domain walk"),
        [None, None]
    );
}

#[test]
fn two_offset_domain_candidates_do_not_build_knot_bands_for_many_queries() {
    use cadmpeg_core::decode::{refusal_probe::RefusalProbe, ResourceDimension};
    let carriers = [
        surface([0.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]),
        surface([0.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]),
    ];
    let queries = vec![offset([0.0, 1.0], [0.0, 1.0]); 8193];
    let _probe = RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "catia_b2_offset_knot_index_scan",
        None,
    );
    let bindings = crate::test_support::with_service_context(|ctx| {
        offset_support_carriers(ctx, &queries, &carriers)
    })
    .expect("a fixed candidate pair needs no knot bands");
    assert_eq!(bindings, vec![None; 8193]);
}

#[test]
fn optional_knot_bands_preserve_rounded_unique_bindings() {
    let carriers = [
        surface(
            [0.0, 1.0],
            vec![
                0.0,
                0.0,
                3.969_197_063_200_492_7,
                100.0,
                30_639.389_180_574_413,
                30_639.389_180_574_413,
            ],
        ),
        surface(
            [0.0, 1.0],
            vec![0.0, 0.0, 30_639.389_180_574_413, 30_639.389_180_574_413],
        ),
        surface(
            [0.0, 1.0],
            vec![0.0, 0.0, 30_639.389_180_574_413, 30_639.389_180_574_413],
        ),
    ];
    // Every domain admits three candidates. 9558*3 exceeds 14*2048, so
    // knot bands run. Only carrier zero has both accepted endpoint knots.
    let queries = vec![offset([0.0, 1.0], [34.608_586_243_774_91, 100.0]); 9558];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        offset_support_carriers(ctx, &queries, &carriers)
    };
    let refused = crate::test_support::with_work_refusal("catia_b2_offset_knot_index_scan", run);
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_b2_offset_knot_index_scan")
    );
    let bindings = crate::test_support::with_service_context(run)
        .expect("rounded knot-band bounds admit the exact match");
    assert_eq!(bindings, vec![Some(0); 9558]);
}
