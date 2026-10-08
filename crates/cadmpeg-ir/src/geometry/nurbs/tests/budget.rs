// SPDX-License-Identifier: Apache-2.0

use crate::geometry::tests::budget::with_limit;
use cadmpeg_core::decode::ResourceDimension;

#[test]
fn curve_pole_validation_stops_before_mutation_on_mapper_refusal() {
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;
    for rational in [false, true] {
        let mut curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0., 0., 1., 1.],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
            rational.then(|| vec![1., 2.]),
            false,
        )
        .expect("admitted fixture or edit")
        .expect("admitted fixture or edit");
        let original = curve.clone();
        assert_eq!(
            with_limit(ResourceDimension::WorkUnits, 4, |ctx| curve
                .try_map_control_points(|_, _| Err("first pole refused"), ctx))
            .expect("admitted fixture or edit"),
            Err("first pole refused")
        );
        assert_eq!(curve, original);
    }
}

#[test]
fn surface_pole_validation_stops_before_mutation_on_mapper_refusal() {
    use crate::geometry::nurbs::{
        BsplineSurface, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
    };
    use crate::math::Point3;
    let points = vec![vec![Point3::new(0., 0., 0.); 2]; 2];
    for rational in [false, true] {
        let mut surface = NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
            NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
            NurbsSurfaceLanes::new(points.clone(), rational.then(|| vec![vec![1.; 2]; 2])),
            false,
        )
        .expect("admitted fixture or edit")
        .expect("admitted fixture or edit");
        let original = surface.clone();
        assert_eq!(
            with_limit(ResourceDimension::WorkUnits, 8, |ctx| surface
                .try_map_control_points(|_, _| Err("first pole refused"), ctx))
            .expect("admitted fixture or edit"),
            Err("first pole refused")
        );
        assert_eq!(surface, original);
    }
    let mut surface = BsplineSurface::new(
        &cadmpeg_test_support::service_decode_context(),
        1,
        1,
        vec![0., 0., 1., 1.],
        vec![0., 0., 1., 1.],
        points,
    )
    .expect("admitted fixture or edit")
    .expect("admitted fixture or edit");
    let original = surface.clone();
    assert_eq!(
        with_limit(ResourceDimension::WorkUnits, 8, |ctx| surface
            .try_map_control_points(|_, _| Err("first pole refused"), ctx))
        .expect("admitted fixture or edit"),
        Err("first pole refused")
    );
    assert_eq!(surface, original);
}
