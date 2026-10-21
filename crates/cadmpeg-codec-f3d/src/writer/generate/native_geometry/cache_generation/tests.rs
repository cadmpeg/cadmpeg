// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::eval::admission::EvaluationAdmission;

fn rational() -> NurbsCurve {
    NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        Some(vec![1.0, 2.0, 1.0]),
        false,
    )
    .unwrap()
}

fn at(surface: &NurbsSurface, u: f64, v: f64) -> Point3 {
    cadmpeg_ir::eval::decode::surface_point_solved(
        EvaluationAdmission::Standard,
        &SolvedSurfaceGeometry::Nurbs(surface.clone()),
        u,
        v,
    )
    .unwrap()
    .get()
}

fn close(actual: Point3, expected: Point3) {
    assert!(
        actual.distance(expected) < 1e-12,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn rational_extrusion_tensor_preserves_weights_and_true_points() {
    let directrix = rational();
    let direction = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 3.0)],
        None,
        false,
    )
    .unwrap();
    let cache = tensor(&directrix, &direction, Vector3::new(0.0, 0.0, 0.0)).unwrap();
    close(at(&cache, 0.5, 0.25), Point3::new(1.0, 4.0 / 3.0, 0.75));
    assert_eq!(
        cache.pole_grid().weights().unwrap(),
        vec![vec![1.0, 1.0], vec![2.0, 2.0], vec![1.0, 1.0]]
    );
}

#[test]
fn rational_sum_tensor_has_independent_directrix_parameters() {
    let cache = tensor(&rational(), &rational(), Vector3::new(3.0, -1.0, 2.0)).unwrap();
    close(
        at(&cache, 0.5, 0.25),
        Point3::new(-15.0 / 11.0, 113.0 / 33.0, -2.0),
    );
    assert_eq!(
        cache.pole_grid().weights().unwrap(),
        vec![
            vec![1.0, 2.0, 1.0],
            vec![2.0, 4.0, 2.0],
            vec![1.0, 2.0, 1.0]
        ]
    );
}

#[test]
fn ruled_cache_joins_two_rational_profiles_without_changing_weights() {
    let mut ir = CadIr::empty();
    let first_id = cadmpeg_ir::ids::CurveId::mint("synthetic:cache-control:curve#first").unwrap();
    let second_id = cadmpeg_ir::ids::CurveId::mint("synthetic:cache-control:curve#second").unwrap();
    let first = rational();
    let mut second = first.clone();
    second
        .try_map_control_points(|_, point| {
            cadmpeg_ir::features::FinitePoint3::new(
                point.get().translated(Vector3::new(0.0, 0.0, 3.0), 1.0),
            )
            .ok_or("finite translated pole")
        })
        .unwrap();
    for (id, curve) in [(first_id.clone(), first), (second_id.clone(), second)] {
        ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
            parameter_range: None,
            source_object: None,
        });
    }
    let construction = ProceduralSurface::new(
        cadmpeg_ir::ids::ProceduralSurfaceId::mint("synthetic:cache-control:construction#ruled")
            .unwrap(),
        ProceduralSurfaceDefinition::Ruled {
            first: first_id,
            second: second_id,
            cache: Some(cadmpeg_ir::geometry::LegacyCache::try_new(0.025).unwrap()),
        },
        None,
    );
    let cache = generate(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &construction,
    )
    .unwrap()
    .unwrap();
    close(at(&cache, 0.5, 0.25), Point3::new(1.0, 4.0 / 3.0, 0.75));
    assert_eq!(construction.cache_fit_tolerance().unwrap().get(), 0.025);
}

fn revolution_control() -> (CadIr, ProceduralSurface) {
    let mut ir = CadIr::empty();
    let id = cadmpeg_ir::ids::CurveId::mint("synthetic:cache-control:curve#revolved").unwrap();
    ir.model.curves.push(cadmpeg_ir::geometry::Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(rational())),
        parameter_range: None,
        source_object: None,
    });
    let payload = cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
        id,
        (
            cadmpeg_ir::features::FinitePoint3::ZERO,
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
        ),
        [0.0, 1.0],
        None,
        Some([0.0, 1.0]),
        false,
        cadmpeg_ir::geometry::CacheContract::Legacy {
            cache: Some(cadmpeg_ir::geometry::LegacyCache::try_new(0.001).unwrap()),
        },
    )
    .unwrap();
    (
        ir,
        ProceduralSurface::new(
            cadmpeg_ir::ids::ProceduralSurfaceId::mint(
                "synthetic:cache-control:construction#revolution",
            )
            .unwrap(),
            ProceduralSurfaceDefinition::Revolution(payload),
            None,
        ),
    )
}

#[test]
fn canonical_revolution_cache_meets_the_actual_fit_between_stations() {
    let (ir, construction) = revolution_control();
    let cache = generate(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &construction,
    )
    .unwrap()
    .unwrap();
    // At u=.5 the independently evaluated rational directrix is (1,4/3,0).
    // The angular values are interior values, not selected cache stations.
    for angle in [0.13_f64, 0.37, 0.71, 0.89] {
        let (sin, cos) = angle.sin_cos();
        let expected = Point3::new(cos - (4.0 / 3.0) * sin, sin + (4.0 / 3.0) * cos, 0.0);
        assert!(at(&cache, 0.5, angle).distance(expected) <= 0.001);
    }
    assert_eq!(construction.cache_fit_tolerance().unwrap().get(), 0.001);
}

#[test]
fn revolution_cache_capacity_refuses_before_adaptive_allocation() {
    let (ir, construction) = revolution_control();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = generate(&ctx, &ir, &construction).unwrap_err();
    assert!(
        matches!(error,CodecError::ResourceLimit(limit) if limit.dimension==cadmpeg_core::decode::ResourceDimension::RetainedBytes && limit.operation=="generate F3D revolution cache output")
    );
}
