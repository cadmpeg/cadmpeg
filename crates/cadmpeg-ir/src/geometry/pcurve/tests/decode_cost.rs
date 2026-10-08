// SPDX-License-Identifier: Apache-2.0

use crate::geometry::pcurve::{
    LinePcurve, OffsetPcurve, PcurveGeometry, PcurveNurbs, PlacedPcurve, TrimmedPcurve,
};
use crate::geometry::tests::budget::with_limit as limited;
use crate::math::Point2;
use crate::transform::Transform2;
use cadmpeg_core::decode::{cost::DecodeCost, ResourceDimension};

#[test]
fn geometry_decode_cost_counts_pcurve_owned_basis_and_poles() {
    let context = cadmpeg_test_support::service_decode_context();
    let nurbs = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            &context,
            1,
            vec![0., 0., 1., 1.],
            vec![Point2::new(0., 0.), Point2::new(1., 1.)],
            Some(vec![1., 2.]),
            false,
        )
        .unwrap()
        .unwrap(),
    };
    // Carrier tag, degree, four knots, pole-form tag, two weighted UV poles, periodicity.
    let expected = 1 + 4 + 4 * 8 + 1 + 2 * (2 * 8 + 8) + 1;
    assert_eq!(
        nurbs.decode_cost(&context, "pcurve cost").unwrap(),
        expected
    );
    let nested = PcurveGeometry::Offset(
        OffsetPcurve::try_new(
            2.,
            Box::new(PcurveGeometry::Trimmed(
                TrimmedPcurve::try_new([0., 1.], true, Box::new(nurbs)).unwrap(),
            )),
        )
        .unwrap(),
    );
    let depth_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>());
    assert_eq!(
        nested.decode_cost(&context, "nested pcurve cost").unwrap(),
        expected + 1 + 8 + depth_bytes + 1 + 2 * 8 + 1 + depth_bytes
    );
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "nested pcurve cost",
        |cap| {
            limited(ResourceDimension::WorkUnits, cap, |ctx| {
                nested.decode_cost(ctx, "nested pcurve cost")
            })
        },
    );
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RecursionDepth,
        "nested pcurve depth",
        |cap| {
            limited(ResourceDimension::RecursionDepth, cap, |ctx| {
                nested.decode_cost(ctx, "nested pcurve depth")
            })
        },
    );
    let placed = PcurveGeometry::Transformed(
        PlacedPcurve::try_new(
            Box::new(PcurveGeometry::Line(LinePcurve::U_AXIS)),
            Transform2::identity(),
        )
        .unwrap(),
    );
    assert_eq!(
        placed.decode_cost(&context, "placed pcurve cost").unwrap(),
        1 + 6 * 8
            + depth_bytes
            + 1
            + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<LinePcurve>())
    );
}

#[test]
fn geometry_decode_cost_counts_both_polar_pole_forms_without_visits() {
    use crate::geometry::pcurve::{PolarNurbsPole, PolarPcurveNurbs};
    for rational in [false, true] {
        let nurbs = PolarPcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0., 0., 1., 1.],
            vec![
                PolarNurbsPole {
                    radial: Point2::new(1., 2.),
                    axial: 3.,
                },
                PolarNurbsPole {
                    radial: Point2::new(4., 5.),
                    axial: 6.,
                },
            ],
            rational.then(|| vec![1., 2.]),
            false,
        )
        .expect("admitted poles")
        .expect("polar NURBS");
        let geometry = PcurveGeometry::PolarNurbs { nurbs };
        assert_eq!(
            limited(ResourceDimension::WorkUnits, 0, |ctx| geometry
                .decode_cost(ctx, "polar pcurve cost"))
            .expect("fixed lane costs"),
            1 + 4 + 4 * 8 + 1 + 2 * (3 * 8 + if rational { 8 } else { 0 }) + 1
        );
    }
}
