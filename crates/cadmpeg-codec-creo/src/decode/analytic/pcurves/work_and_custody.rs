// SPDX-License-Identifier: Apache-2.0

use super::{
    map_pcurve_paths, planar_curve_pcurve, unique_oriented_native_pcurve, OrientedNativePcurve,
    SurfaceIndex,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, WeightedPole2};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::units::FinitePoint2;
use std::collections::BTreeMap;
use std::mem::size_of;

const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;
const PARENT_BYTES: u64 = 37;
const PROJECTED_POLE_SLOTS: usize = 4;
const KNOT_SLOTS: usize = 4;

fn plane() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
    ))
}

fn source_nurbs(rational: bool) -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        crate::decode::with_test_decode_ctx(|ctx| {
            NurbsCurve::from_lanes(
                ctx,
                1,
                vec![2.0, 2.0, 5.0, 5.0],
                vec![Point3::new(2.0, 4.0, 3.0), Point3::new(5.0, 7.0, 3.0)],
                rational.then(|| vec![2.0, 1.0]),
                false,
            )
        })
        .expect("source construction admission")
        .expect("source NURBS"),
    ))
}

fn projection(
    ctx: &DecodeContext<'_>,
    source: &CurveGeometry,
) -> Result<PcurveGeometry, CodecError> {
    let mut refusals = crate::lane_refusal::LaneRefusals::new();
    let result = planar_curve_pcurve(ctx, &plane(), source, &"planar NURBS", &mut refusals)?;
    assert!(refusals.take_records_checked()?.is_empty());
    Ok(result.expect("planar source"))
}

fn assert_projection(value: &PcurveGeometry, rational: bool) {
    let PcurveGeometry::Nurbs { nurbs } = value else {
        panic!("projected NURBS")
    };
    assert_eq!(nurbs.degree(), 1);
    assert_eq!(nurbs.knots().as_slice(), [2.0, 2.0, 5.0, 5.0]);
    assert_eq!(
        nurbs.control_points(),
        [Point2::new(2.0, 4.0), Point2::new(5.0, 7.0)]
    );
    assert_eq!(
        nurbs.pole_rows().weights(),
        rational.then(|| vec![2.0, 1.0])
    );
    assert!(!nurbs.periodic());
}

fn fixed_absent_paths(count: usize) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let ir = CadIr::empty();
    let index = SurfaceIndex::new(&ctx, &[]).expect("empty index");
    let paths = [0, 1].map(|side| (side < count).then_some((None, [[0.0; 2]; 2])));
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let result = map_pcurve_paths(&ctx, &ir, paths, &carriers, &index).expect("fixed paths");
    assert_eq!(result.missing_surfaces, count);
    assert_eq!(result.unevaluable_paths, 0);
    assert!(result.mapped.iter().all(Option::is_none));
    let original = ctx
        .charge_work_limit(1, "before fixed path mapping")
        .expect_err("zero work");
    assert!(
        matches!(map_pcurve_paths(&ctx, &ir, paths, &carriers, &index),
        Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
}

#[test]
fn empty_fixed_pcurve_paths_are_free_and_keep_original_refusal() {
    fixed_absent_paths(0);
}

#[test]
fn one_absent_face_pcurve_path_is_free_and_keeps_original_refusal() {
    fixed_absent_paths(1);
}

#[test]
fn two_absent_face_pcurve_paths_are_free_and_keep_original_refusal() {
    fixed_absent_paths(2);
}

#[test]
fn oriented_pcurve_candidates_admit_only_present_rows() {
    let endpoints = [[2.0, 4.0], [5.0, 7.0]];
    let points = [[2.0, 4.0, 3.0], [5.0, 7.0, 3.0]];
    for count in 0_u32..=3 {
        let candidates: Vec<_> = (0..count)
            .map(|index| (endpoints, usize::try_from(3 - index).expect("three rows")))
            .collect();
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &vec![
                "creo oriented native pcurve candidates";
                usize::try_from(count).expect("three rows")
            ],
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_retained_bytes = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = unique_oriented_native_pcurve(&ctx, &plane(), &candidates, points);
                if let Err(CodecError::ResourceLimit(original)) = &result {
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(original.operation, "creo oriented native pcurve candidates");
                    assert_eq!((original.used, original.additional), (cap, 1));
                    assert!(
                        matches!(unique_oriented_native_pcurve(&ctx, &plane(), &[], points),
                    Err(CodecError::ResourceLimit(actual)) if actual == *original)
                    );
                    return result.map(|_| ());
                }
                assert_eq!(cap, u64::from(count));
                assert_eq!(
                    result.expect("exact row work"),
                    (count > 0).then(|| OrientedNativePcurve {
                        endpoints,
                        offset: usize::try_from(4 - count).expect("three rows"),
                    })
                );
                assert!(ctx.resource_refusal().is_none());
                if count == 0 {
                    let original = ctx
                        .charge_work_limit(1, "before empty oriented candidates")
                        .expect_err("zero work");
                    assert!(
                        matches!(unique_oriented_native_pcurve(&ctx, &plane(), &[], points),
                    Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                }
                Ok(())
            },
        );
    }
}

fn assert_projection_work(rational: bool) {
    let source = source_nurbs(rational);
    let pole_operation = if rational {
        "creo NURBS rational poles"
    } else {
        "creo NURBS polynomial poles"
    };
    let projection_operation = if rational {
        "creo planar rational NURBS poles"
    } else {
        "creo planar polynomial NURBS poles"
    };
    let value = crate::test_support::assert_work_boundaries(
        &[
            pole_operation,
            projection_operation,
            "creo planar projected NURBS knots",
        ],
        |ctx| projection(ctx, &source),
    );
    assert_projection(&value, rational);
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits,
        None,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            match projection(&ctx, &source) {
                Err(CodecError::ResourceLimit(original)) => {
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert!(
                        matches!(projection(&ctx, &source), Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                    Err(CodecError::ResourceLimit(original))
                }
                result => result,
            }
        },
    );
    // Two extent poles, two projected poles, and four copied knots.
    assert_eq!(limit, 8);
}

#[test]
fn polynomial_planar_projection_admits_only_source_work() {
    assert_projection_work(false);
}

#[test]
fn rational_planar_projection_admits_only_source_work() {
    assert_projection_work(true);
}

#[test]
fn planar_projection_retains_only_surviving_poles_and_knots() {
    for rational in [false, true] {
        let source = source_nurbs(rational);
        let pole_size = if rational {
            size_of::<WeightedPole2<FinitePoint2>>()
        } else {
            size_of::<FinitePoint2>()
        };
        let pole_bytes = u64::try_from(PROJECTED_POLE_SLOTS * pole_size).expect("four pole slots");
        let knot_bytes = u64::try_from(KNOT_SLOTS * size_of::<f64>()).expect("four knots");
        let bytes = pole_bytes + knot_bytes;
        crate::test_support::assert_refusal_order(
            ResourceDimension::RetainedBytes,
            &[
                "creo planar projected NURBS poles",
                "creo planar projected NURBS knots",
            ],
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = projection(&ctx, &source);
                let original = if let Ok(value) = result {
                    assert_eq!(cap, bytes);
                    assert_projection(&value, rational);
                    let original = ctx
                        .charge_retained_limit(1, "after planar output backing")
                        .expect_err("exact output cap");
                    assert_eq!(original.operation, "after planar output backing");
                    assert_eq!((original.used, original.additional), (bytes, 1));
                    original
                } else {
                    let Err(CodecError::ResourceLimit(original)) = result else {
                        panic!("output storage bound")
                    };
                    let (operation, used, additional) = if cap < pole_bytes {
                        ("creo planar projected NURBS poles", 0, pole_bytes)
                    } else {
                        ("creo planar projected NURBS knots", pole_bytes, knot_bytes)
                    };
                    assert_eq!(original.operation, operation);
                    assert_eq!((original.used, original.additional), (used, additional));
                    original
                };
                assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
                assert!(
                    matches!(projection(&ctx, &source), Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                if cap == bytes {
                    Ok(())
                } else {
                    Err(CodecError::ResourceLimit(original))
                }
            },
        );
    }
}

#[test]
fn planar_projection_transfers_actual_output_to_its_live_parent() {
    for rational in [false, true] {
        let source = source_nurbs(rational);
        let pole_size = if rational {
            size_of::<WeightedPole2<FinitePoint2>>()
        } else {
            size_of::<FinitePoint2>()
        };
        let bytes = u64::try_from(PROJECTED_POLE_SLOTS * pole_size + KNOT_SLOTS * size_of::<f64>())
            .expect("output backing");
        for refuse in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut parent = ctx
                .reserve_scoped(PARENT_BYTES, "planar output parent")
                .expect("parent");
            let value = parent
                .with_storage(|| projection(&ctx, &source))
                .expect("pure output transfer");
            assert_projection(&value, rational);
            let remaining = EMPTY_ROOT_MATERIALIZED_BYTES - PARENT_BYTES - bytes;
            if refuse {
                let original = ctx
                    .reserve_scoped_limit(remaining + 1, "live planar output overlap")
                    .expect_err("one over exact overlap");
                assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(
                    (original.used, original.additional),
                    (PARENT_BYTES + bytes, remaining + 1)
                );
                assert_eq!(ctx.resource_refusal(), Some(original));
            } else {
                let rest = ctx
                    .reserve_scoped_limit(remaining, "live planar output overlap")
                    .expect("exact overlap");
                drop(rest);
            }
            drop(value);
            drop(parent);
            if !refuse {
                let full = ctx
                    .reserve_scoped_limit(EMPTY_ROOT_MATERIALIZED_BYTES, "after planar output drop")
                    .expect("all scratch released");
                drop(full);
                ctx.finish_session()
                    .expect("no retained output outside parent");
            }
        }
    }
}

#[test]
fn empty_pcurve_domain_propagation_needs_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let solved = super::solve_pcurve_vertex_domains(
        &ctx,
        &[],
        &BTreeMap::default(),
        &BTreeMap::default(),
        &BTreeMap::default(),
    )
    .expect("empty propagation");
    assert!(solved.is_empty());
}

#[test]
fn mapped_pcurve_endpoint_agreement_needs_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let points = [[0.0; 3], [1.0, 0.0, 0.0]];
    let path = super::MappedPcurvePath {
        face_id: 1,
        endpoints: points,
    };
    for paths in [
        [Some(path), None],
        [None, Some(path)],
        [Some(path), Some(path)],
    ] {
        let evidence = super::pcurve_endpoint_evidence_from_mapped(&ctx, &paths, true)
            .expect("fixed agreement")
            .expect("finite paths agree");
        assert_eq!(evidence.points, points);
        assert_eq!(evidence.complete, paths.iter().flatten().count() == 2);
        assert!(evidence.authoritative);
    }
}

#[test]
fn repeated_pcurve_propagation_reuses_live_domain_backing() {
    let a = [1.0, 0.0, 0.0];
    let b = [2.0, 0.0, 0.0];
    let constraints = vec![([1, 2], [a, b]); 128];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let solved = super::solve_pcurve_vertex_domains(
        &ctx,
        &constraints,
        &BTreeMap::from([(1, a), (2, b)]),
        &BTreeMap::default(),
        &BTreeMap::default(),
    )
    .expect("live storage remains bounded");
    assert_eq!(solved, BTreeMap::from([(1, a), (2, b)]));
    ctx.reserve_scoped(4096, "released propagation scratch")
        .expect("all scratch released");
}
