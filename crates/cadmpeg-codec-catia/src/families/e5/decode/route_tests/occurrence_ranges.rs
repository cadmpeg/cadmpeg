// SPDX-License-Identifier: Apache-2.0

use crate::assemble::quintic_jet_pcurve;
use crate::families::e5::decode::{
    parameter_range_agreement_tolerance, EPS_E5_DECODE_EXACT_GEOMETRY,
};

use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

#[test]
fn support_range_agreement_requires_matching_endpoints() {
    assert!(parameter_range_agreement_tolerance([0.0, 5.0], [0.0, 5.0]).is_some());
    assert!(parameter_range_agreement_tolerance([0.0, 5.0], [1.0, 6.0]).is_none());
}

#[test]
fn quintic_jet_reproduces_endpoint_second_order_data() {
    let curve = quintic_jet_pcurve(
        5,
        &[0.0, 2.0],
        &[[0.0, 0.0], [2.0, 0.0]],
        (&[[1.0, 0.0], [1.0, 0.0]], &[[0.0, 0.0], [0.0, 0.0]]),
        &mut crate::nurbs::LaneRefusals::new(),
        "test record",
    )
    .expect("linear quintic segment");
    for parameter in [0.0, 0.5, 1.0, 2.0] {
        let point = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &curve,
            parameter,
        )
        .expect("jet evaluation");
        assert!((point.u - parameter).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!(point.v.abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    }
}

#[test]
fn reversing_nurbs_preserves_tiny_knot_domain() {
    let tiny = 1e-200;
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![tiny, tiny, 2.0 * tiny, 2.0 * tiny],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid tiny-domain NURBS"),
    ));
    let (reversed, range) = crate::nurbs::reverse_curve_geometry(
        &curve,
        [tiny, 2.0 * tiny],
        &mut crate::nurbs::LaneRefusals::new(),
        "test record",
    )
    .expect("reversed NURBS");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(reversed)) = reversed else {
        panic!("expected NURBS");
    };
    assert_eq!(range, [tiny, 2.0 * tiny]);
    assert_eq!(
        reversed.knots().as_slice(),
        [tiny, tiny, 2.0 * tiny, 2.0 * tiny]
    );
    assert_eq!(
        reversed.control_points(),
        [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)]
    );
}
