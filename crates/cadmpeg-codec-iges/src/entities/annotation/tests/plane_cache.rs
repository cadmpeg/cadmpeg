// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn sectioned_area_plane_cache_distinguishes_rounded_offset_collisions() {
    let origin = Point3::new(2.0_f64.powi(60), 0.0, 0.0);
    let normal = Vector3::new(1.0, 1.0, 0.0);
    let shifted = Point3::new(origin.x, 1.0, 0.0);
    assert_eq!(origin.vector_from(Point3::new(0.0, 0.0, 0.0)).dot(normal),
        shifted.vector_from(Point3::new(0.0, 0.0, 0.0)).dot(normal));
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("iges:model:curve#D1").unwrap(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(origin, normal.unit().unwrap(),
                Vector3::new(0.0, 0.0, 1.0), 1.0).unwrap())),
        source_object: None,
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let mut cache = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
        assert!(cache.curve_coplanar(1, (origin, normal), 0.001, ctx).unwrap());
        assert!(!cache.curve_coplanar(1, (shifted, normal), 0.001, ctx).unwrap());
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RecursionDepth, "iges coplanar curve recursion", None);
        assert!(cache.curve_coplanar(1, (origin, normal), 0.001, ctx).unwrap());
        assert!(!cache.curve_coplanar(1, (shifted, normal), 0.001, ctx).unwrap());
    });
}

#[test]
fn sectioned_area_plane_cache_reuses_equivalent_overflowed_offsets() {
    let origin = Point3::new(1.5e308, 1.5e308, 0.0);
    let normal = Vector3::new(1.0, 1.0, 0.0);
    let tangent = 2.0_f64.powi(1000);
    let shifted = Point3::new(origin.x + tangent, origin.y - tangent, 0.0);
    assert!(origin.vector_from(Point3::new(0.0, 0.0, 0.0)).dot(normal).is_infinite());
    assert_eq!(shifted.vector_from(origin).dot(normal), 0.0);
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: CurveId::mint("iges:model:curve#D1").unwrap(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(origin, normal.unit().unwrap(),
                Vector3::new(0.0, 0.0, 1.0), 1.0).unwrap())),
        source_object: None,
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let mut cache = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
        assert!(cache.curve_coplanar(1, (origin, normal), 0.001, ctx).unwrap());
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RecursionDepth, "iges coplanar curve recursion", None);
        assert!(cache.curve_coplanar(1, (shifted, normal), 0.001, ctx).unwrap());
        assert!(cache.curve_coplanar(1, (shifted, normal.scale(-1.0)), 0.001, ctx).unwrap());
    });
}

fn linear_nurbs_for_cache(points: Vec<Point3>) -> SolvedCurveGeometry {
    crate::test_support::with_service_context(&[], |ctx| {
        SolvedCurveGeometry::Nurbs(cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            ctx, 1, vec![0.0, 0.0, 1.0, 1.0], points, None, false).unwrap().unwrap())
    })
}

#[test]
fn sectioned_area_plane_cache_preserves_origin_rounding_results() {
    let center = Point3::new(1.0e16, -1.0e16 + 2.0, 0.0);
    let normal = Vector3::new(1.0, 1.0, 0.0).unit().unwrap();
    let original = (Point3::new(0.0, 0.0, 0.0), normal);
    let shifted = (Point3::new(1.0, -1.0, 0.0), normal);
    for geometry in [
        SolvedCurveGeometry::Circle(cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            center, normal, Vector3::new(0.0, 0.0, 1.0), 1.0).unwrap()),
        linear_nurbs_for_cache(vec![center, Point3::new(center.x, center.y, 1.0)]),
    ] {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve { id: CurveId::mint("iges:model:curve#D1").unwrap(),
            geometry: CurveGeometry::Solved(geometry), source_object: None });
        crate::test_support::with_service_context(&[], |ctx| {
            let mut cached = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
            assert!(cached.curve_coplanar(1, original, 2.0, ctx).unwrap());
            let mut fresh = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
            let uncached = fresh.curve_coplanar(1, shifted, 2.0, ctx).unwrap();
            assert!(!uncached);
            assert_eq!(cached.curve_coplanar(1, shifted, 2.0, ctx).unwrap(), uncached);
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::RecursionDepth, "iges coplanar curve recursion", None);
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
        SolvedCurveGeometry::Circle(cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            center, normal, Vector3::new(1.0, 0.0, 0.0), 1.0).unwrap()),
        linear_nurbs_for_cache(vec![center, Point3::new(center.x, 1.0, 0.0)]),
    ] {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve { id: CurveId::mint("iges:model:curve#D1").unwrap(),
            geometry: CurveGeometry::Solved(geometry), source_object: None });
        crate::test_support::with_service_context(&[], |ctx| {
            let mut cached = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
            assert!(cached.curve_coplanar(1, original, 0.001, ctx).unwrap());
            let mut fresh = super::super::SectionedAreaGeometryCache::new(&ir, ctx).unwrap();
            let uncached = fresh.curve_coplanar(1, shifted, 0.001, ctx).unwrap();
            assert!(!uncached);
            assert_eq!(cached.curve_coplanar(1, shifted, 0.001, ctx).unwrap(), uncached);
        });
    }
}
