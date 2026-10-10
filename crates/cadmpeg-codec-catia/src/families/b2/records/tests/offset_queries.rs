// SPDX-License-Identifier: Apache-2.0
//! Offset candidate indexing and consumer admission.

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
fn circles_without_revolutions_build_no_profile_index() {
    let bytes = crate::test_support::test_b2::b2_circle_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert!(crate::test_support::with_materialized_limit(0, |ctx| {
        super::super::b2_resolved_revolutions_from_records(ctx, &bytes, &records)
    })
    .expect("no profile consumer")
    .is_empty());
}
