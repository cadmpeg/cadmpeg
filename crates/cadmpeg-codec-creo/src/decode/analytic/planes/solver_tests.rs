use super::solve_carriers_with_diagnostics;
use crate::decode::analytic::equations::{CarrierEquation, PlaneEquation, SphereEquation};

fn tangent_pair() -> [CarrierEquation; 2] {
    [
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
        }),
        CarrierEquation::Sphere(SphereEquation {
            center: [0.0, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 1.0,
        }),
    ]
}

fn orthogonal_planes() -> [CarrierEquation; 3] {
    [
        CarrierEquation::Plane(PlaneEquation {
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        }),
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 2.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        }),
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0, 0.0, 3.0],
            normal: [0.0, 0.0, 1.0],
        }),
    ]
}

fn limit_error(carriers: &[CarrierEquation], operation: &'static str) {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |ctx| solve_carriers_with_diagnostics(ctx, carriers),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource) if resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn carrier_pair_candidates_refuse_collection_limit() {
    limit_error(&tangent_pair(), "creo carrier pair candidates");
}

#[test]
fn carrier_triple_candidates_refuse_collection_limit() {
    limit_error(&orthogonal_planes(), "creo carrier triple candidates");
}

#[test]
fn carrier_unique_candidates_refuse_collection_limit() {
    limit_error(&tangent_pair(), "creo carrier unique candidates");
}

#[test]
fn carrier_solver_keeps_pair_and_triple_solutions_under_service_policy() {
    crate::decode::with_test_decode_ctx(|ctx| {
        assert_eq!(
            solve_carriers_with_diagnostics(ctx, &tangent_pair())?.0,
            Some([0.0, 0.0, 1.0])
        );
        assert_eq!(
            solve_carriers_with_diagnostics(ctx, &orthogonal_planes())?.0,
            Some([1.0, 2.0, 3.0])
        );
        Ok::<_, cadmpeg_core::CodecError>(())
    })
    .expect("service profile admits both carrier witnesses");
}

#[test]
fn repeated_parallel_carrier_triples_use_no_heap_groups() {
    let planes = [orthogonal_planes()[0]; 82];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (point, diagnostics) =
        solve_carriers_with_diagnostics(&ctx, &planes).expect("fixed triple grouping");
    assert_eq!(point, None);
    assert_eq!(diagnostics, super::CarrierSolveDiagnostics::default());
}

#[test]
fn nurbs_plane_fold_visits_first_pole_before_allocation_refusal() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::Point3;
    for rational in [false, true] {
        let count = if rational { 256_u16 } else { 4096 };
        let mut knots = vec![0.0, 0.0];
        knots.extend((1..count).map(f64::from));
        knots.push(f64::from(count - 1));
        let count = usize::from(count);
        let nurbs = crate::decode::with_test_decode_ctx(|ctx| {
            NurbsCurve::from_lanes(
                ctx,
                1,
                knots,
                vec![Point3::new(0.0, 0.0, 0.0); count],
                rational.then(|| vec![1.0; count]),
                false,
            )
        })
        .expect("constructor admission")
        .expect("NURBS");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let run = |cap| {
            let mut policy = policy;
            policy.limits.max_work_units = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            super::analytic_curve_plane(
                &ctx,
                &CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs.clone())),
            )
        };
        // Rational weight validation is a separate source traversal.
        let resource = crate::test_support::assert_refusal_order(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            &[],
            |cap| match run(cap) {
                Err(cadmpeg_core::CodecError::ResourceLimit(resource))
                    if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits =>
                {
                    assert_eq!(
                        resource.additional, 1,
                        "only the next pole visit is charged"
                    );
                    Err(cadmpeg_core::CodecError::ResourceLimit(resource))
                }
                Err(cadmpeg_core::CodecError::ResourceLimit(resource)) => Ok(resource),
                _ => panic!("first pole storage must refuse"),
            },
        );
        assert_eq!(
            resource.dimension,
            cadmpeg_core::decode::ResourceDimension::CollectionItems
        );
        assert_eq!(resource.operation, "creo topology plane candidate points");
        assert_eq!((resource.used, resource.additional), (0, 1));
    }
}

#[test]
fn repeated_intersecting_triples_release_probe_backing() {
    use super::super::equations::{CarrierEquation, PlaneEquation, SphereEquation};
    let mut carriers = vec![
        CarrierEquation::Plane(PlaneEquation {
            origin: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
        });
        24
    ];
    carriers.extend([0.0, 1.0].map(|x| {
        CarrierEquation::Sphere(SphereEquation {
            center: [x, 0.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 1.0,
        })
    }));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = 3072;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (point, diagnostics) = solve_carriers_with_diagnostics(&ctx, &carriers)
        .expect("only live probe storage is charged");
    assert_eq!(point, None);
    assert_eq!(diagnostics.pair_intersections, 0);
    assert_eq!(diagnostics.triple_intersections, 48);
    assert_eq!(diagnostics.valid_candidates, 48);
    assert_eq!(diagnostics.unique_solutions, 2);
    ctx.reserve_scoped(3072, "released triple scratch")
        .expect("all scratch released");
}
