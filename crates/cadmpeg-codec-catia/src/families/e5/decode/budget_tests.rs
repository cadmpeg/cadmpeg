// SPDX-License-Identifier: Apache-2.0
//! E5 geometry equality and scratch ownership.

use super::*;

fn curve(points: usize, rational: bool) -> CurveGeometry {
    let mut knots = vec![0.0, 0.0];
    knots
        .extend((1..points - 1).map(|index| f64::from(u32::try_from(index).expect("small index"))));
    let end = f64::from(u32::try_from(points - 1).expect("small index"));
    knots.extend([end, end]);
    let controls = (0..points)
        .map(|index| {
            Point3::new(
                f64::from(u32::try_from(index).expect("small index")),
                0.0,
                0.0,
            )
        })
        .collect();
    let weights = rational.then(|| vec![1.0; points]);
    let nurbs = crate::test_support::with_service_context(|ctx| {
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(ctx, 1, knots, controls, weights, false)
    })
    .expect("construction budget")
    .expect("valid open curve");
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
}

#[test]
fn e5_equal_nurbs_carriers_charge_the_knot_and_pole_lanes() {
    for rational in [false, true] {
        let left = curve(256, rational);
        let right = left.clone();
        assert!(crate::test_support::with_service_context(|ctx| {
            equivalent_e5_curve_carriers(ctx, &left, &right)
        })
        .expect("equal carriers"));
        let refused =
            crate::test_support::with_work_refusal("catia_e5_curve_geometry_compare", |ctx| {
                equivalent_e5_curve_carriers(ctx, &left, &right)
            });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_curve_geometry_compare")
        );
    }
}

#[test]
fn e5_boundary_pcurve_equality_charges_nurbs_lanes() {
    let left = crate::test_support::with_service_context(|ctx| {
        PcurveNurbs::from_lanes(
            ctx,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
    })
    .expect("construction budget")
    .expect("valid rational pcurve");
    let left = PcurveGeometry::Nurbs { nurbs: left };
    let right = left.clone();
    assert!(crate::test_support::with_service_context(|ctx| {
        equal_e5_pcurve_geometry(ctx, &left, &right)
    })
    .expect("equal pcurves"));
    assert!(
        matches!(crate::test_support::with_work_refusal("catia_e5_pcurve_geometry_compare", |ctx| {
        equal_e5_pcurve_geometry(ctx, &left, &right)
    }), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_pcurve_geometry_compare")
    );
}
