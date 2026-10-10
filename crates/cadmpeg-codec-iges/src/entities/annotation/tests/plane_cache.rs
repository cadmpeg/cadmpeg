// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn sectioned_area_plane_cache_distinguishes_rounded_offset_collisions() {
    let origin = Point3::new(2.0_f64.powi(60), 0.0, 0.0);
    let normal = Vector3::new(1.0, 1.0, 0.0);
    let shifted = Point3::new(origin.x, 1.0, 0.0);
    assert_eq!(
        origin.vector_from(Point3::new(0.0, 0.0, 0.0)).dot(normal),
        shifted.vector_from(Point3::new(0.0, 0.0, 0.0)).dot(normal)
    );
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("iges:model:curve#D1").unwrap(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                origin,
                normal.unit().unwrap(),
                Vector3::new(0.0, 0.0, 1.0),
                1.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let mut cache = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
        assert!(cache
            .curve_coplanar(1, (origin, normal), 0.001, ctx)
            .unwrap());
        assert!(!cache
            .curve_coplanar(1, (shifted, normal), 0.001, ctx)
            .unwrap());
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RecursionDepth,
            "iges coplanar curve recursion",
            None,
        );
        assert!(cache
            .curve_coplanar(1, (origin, normal), 0.001, ctx)
            .unwrap());
        assert!(!cache
            .curve_coplanar(1, (shifted, normal), 0.001, ctx)
            .unwrap());
    });
}

#[test]
fn sectioned_area_plane_cache_reuses_equivalent_overflowed_offsets() {
    let origin = Point3::new(1.5e308, 1.5e308, 0.0);
    let normal = Vector3::new(1.0, 1.0, 0.0);
    let tangent = 2.0_f64.powi(1000);
    let shifted = Point3::new(origin.x + tangent, origin.y - tangent, 0.0);
    assert!(origin
        .vector_from(Point3::new(0.0, 0.0, 0.0))
        .dot(normal)
        .is_infinite());
    assert_eq!(shifted.vector_from(origin).dot(normal), 0.0);
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("iges:model:curve#D1").unwrap(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                origin,
                normal.unit().unwrap(),
                Vector3::new(0.0, 0.0, 1.0),
                1.0,
            )
            .unwrap(),
        )),
        source_object: None,
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let mut cache = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
        assert!(cache
            .curve_coplanar(1, (origin, normal), 0.001, ctx)
            .unwrap());
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RecursionDepth,
            "iges coplanar curve recursion",
            None,
        );
        assert!(cache
            .curve_coplanar(1, (shifted, normal), 0.001, ctx)
            .unwrap());
        assert!(cache
            .curve_coplanar(1, (shifted, normal.scale(-1.0)), 0.001, ctx)
            .unwrap());
    });
}

fn linear_nurbs_for_cache(points: Vec<Point3>) -> SolvedCurveGeometry {
    crate::test_support::with_service_context(&[], |ctx| {
        SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                ctx,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                points,
                None,
                false,
            )
            .unwrap()
            .unwrap(),
        )
    })
}

#[test]
fn sectioned_area_plane_cache_preserves_origin_rounding_results() {
    let center = Point3::new(1.0e16, -1.0e16 + 2.0, 0.0);
    let normal = Vector3::new(1.0, 1.0, 0.0).unit().unwrap();
    let original = (Point3::new(0.0, 0.0, 0.0), normal);
    let shifted = (Point3::new(1.0, -1.0, 0.0), normal);
    for geometry in [
        SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                center,
                normal,
                Vector3::new(0.0, 0.0, 1.0),
                1.0,
            )
            .unwrap(),
        ),
        linear_nurbs_for_cache(vec![center, Point3::new(center.x, center.y, 1.0)]),
    ] {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            id: CurveId::mint("iges:model:curve#D1").unwrap(),
            geometry: CurveGeometry::Solved(geometry),
            source_object: None,
        });
        crate::test_support::with_service_context(&[], |ctx| {
            let mut cached = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
            assert!(cached.curve_coplanar(1, original, 2.0, ctx).unwrap());
            let mut fresh = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
            let uncached = fresh.curve_coplanar(1, shifted, 2.0, ctx).unwrap();
            assert!(!uncached);
            assert_eq!(
                cached.curve_coplanar(1, shifted, 2.0, ctx).unwrap(),
                uncached
            );
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::RecursionDepth,
                "iges coplanar curve recursion",
                None,
            );
            assert!(cached.curve_coplanar(1, original, 2.0, ctx).unwrap());
            assert!(!cached.curve_coplanar(1, shifted, 2.0, ctx).unwrap());
        });
    }
}

#[test]
fn sectioned_area_plane_cache_preserves_origin_subtraction_overflow() {
    let center = Point3::new(-f64::MAX, 0.0, 0.0);
    let normal = Vector3::new(0.0, 0.0, 1.0);
    let original = (Point3::new(0.0, 0.0, 0.0), normal);
    let shifted = (Point3::new(f64::MAX, 0.0, 0.0), normal);
    for geometry in [
        SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                center,
                normal,
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .unwrap(),
        ),
        linear_nurbs_for_cache(vec![center, Point3::new(center.x, 1.0, 0.0)]),
    ] {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            id: CurveId::mint("iges:model:curve#D1").unwrap(),
            geometry: CurveGeometry::Solved(geometry),
            source_object: None,
        });
        crate::test_support::with_service_context(&[], |ctx| {
            let mut cached = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
            assert!(cached.curve_coplanar(1, original, 0.001, ctx).unwrap());
            let mut fresh = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
            let uncached = fresh.curve_coplanar(1, shifted, 0.001, ctx).unwrap();
            assert!(!uncached);
            assert_eq!(
                cached.curve_coplanar(1, shifted, 0.001, ctx).unwrap(),
                uncached
            );
        });
    }
}

#[test]
fn sectioned_area_plane_cache_reuses_oblique_translations_and_scaled_normals() {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("iges:model:curve#D1").unwrap(),
        geometry: CurveGeometry::Solved(linear_nurbs_for_cache(vec![
            Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, -1.0, 1.0),
        ])), source_object: None,
    });
    let normal = Vector3::new(1.0, 1.0, 0.0).unit().unwrap();
    crate::test_support::with_service_context(&[], |ctx| {
        let mut cache = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
        assert!(cache.curve_coplanar(1, (Point3::new(0.0, 0.0, 0.0), normal), 0.001, ctx).unwrap());
        {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::RecursionDepth, "iges coplanar curve recursion", None,
            );
            for index in 0..2000 {
                let coordinate = f64::from(index);
                let origin = Point3::new(coordinate, -coordinate, coordinate / 2.0);
                let normal = normal.scale(if index % 2 == 0 { 3.0 } else { -2.0 });
                assert!(cache.curve_coplanar(1, (origin, normal), 0.001, ctx).unwrap());
            }
        }
        assert_eq!(cache.proofs.len(), 1);
        assert!(!cache.curve_coplanar(1, (Point3::new(0.001, 0.001, 0.0), normal), 0.001, ctx).unwrap());
    });
}

#[test]
fn sectioned_area_plane_cache_does_not_reuse_a_nearby_plane_across_the_distance_limit() {
    let normal = Vector3::new(1.0, 1.0, 0.0).unit().unwrap();
    let center = Point3::new(0.00095 * normal.x, 0.00095 * normal.y, 0.0);
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("iges:model:curve#D1").unwrap(),
        geometry: CurveGeometry::Solved(linear_nurbs_for_cache(vec![center, Point3::new(center.x, center.y, 1.0)])),
        source_object: None,
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let mut cache = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
        let original = (Point3::new(0.0, 0.0, 0.0), normal);
        let outside = (Point3::new(-0.0001 * normal.x, -0.0001 * normal.y, 0.0), normal);
        assert!(cache.curve_coplanar(1, original, 0.001, ctx).unwrap());
        let mut fresh = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
        let expected = fresh.curve_coplanar(1, outside, 0.001, ctx).unwrap();
        assert!(!expected);
        assert_eq!(cache.curve_coplanar(1, outside, 0.001, ctx).unwrap(), expected);
        assert!(cache.curve_coplanar(1, original, 0.001, ctx).unwrap());
    });
}

#[test]
fn sectioned_area_plane_cache_rechecks_failed_control_points_after_normal_scaling() {
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("iges:model:curve#D1").unwrap(),
        geometry: CurveGeometry::Solved(linear_nurbs_for_cache(vec![
            Point3::new(0.0, 0.0, 0.00075), Point3::new(1.0, 0.0, 0.00075),
        ])), source_object: None,
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let mut cache = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
        assert!(!cache.curve_coplanar(1, (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 2.0)), 0.001, ctx).unwrap());
        assert!(cache.curve_coplanar(1, (Point3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)), 0.001, ctx).unwrap());
    });
}
