// SPDX-License-Identifier: Apache-2.0
//! Fixed plane solves preserve refusal; variable rosters admit their scans.

use super::super::{solve_planes, PlaneEquation};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const PLANES: [PlaneEquation; 3] = [
    PlaneEquation { origin: [1.0, 0.0, 0.0], normal: [1.0, 0.0, 0.0] },
    PlaneEquation { origin: [0.0, 2.0, 0.0], normal: [0.0, 1.0, 0.0] },
    PlaneEquation { origin: [0.0, 0.0, 3.0], normal: [0.0, 0.0, 1.0] },
];

#[test]
fn plane_solver_refuses_candidate_scan_work() {
    let planes = [PLANES[0], PLANES[1], PLANES[2], PLANES[0]];
    let point = crate::test_support::assert_work_boundaries(
        &["creo plane solver candidates"],
        |ctx| solve_planes(ctx, &planes),
    );
    assert_eq!(point, Some([1.0, 2.0, 3.0]));
}

#[test]
fn fixed_plane_solver_preserves_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "prior plane work")
        .expect_err("seed refusal") else {
        panic!("resource refusal");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    for count in 0..=PLANES.len() {
        assert!(matches!(solve_planes(&ctx, &PLANES[..count]),
            Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    }
}

#[test]
fn incomplete_plane_rosters_need_no_scan_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for count in 0..PLANES.len() {
        assert_eq!(solve_planes(&ctx, &PLANES[..count]).expect("no plane triple"), None);
    }
}

#[test]
fn variable_plane_solver_exhaustion_admits_only_existing_candidates() {
    let planes = [PLANES[0]; 4];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        solve_planes(&ctx, &planes)
    };
    let limit = crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, run);
    // Four first candidates, six second candidates and four third candidates.
    assert_eq!(limit, 14);
    let error = crate::test_support::last_refusal_at(&[], ResourceDimension::WorkUnits,
        "creo plane solver candidates", |ctx| solve_planes(ctx, &planes));
    assert!(matches!(error, CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::WorkUnits
            && refusal.operation == "creo plane solver candidates"
            && refusal.used == limit - 1 && refusal.additional == 1));
}

#[test]
fn variable_plane_solver_residual_exhaustion_admits_only_existing_planes() {
    let planes = [PLANES[0], PLANES[1], PLANES[2], PLANES[0]];
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let result = solve_planes(&ctx, &planes);
        if let Ok(point) = result { assert_eq!(point, Some([1.0, 2.0, 3.0])); }
        result
    };
    let limit = crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, run);
    // Three candidate visits and four residuals.
    assert_eq!(limit, 7);
}
