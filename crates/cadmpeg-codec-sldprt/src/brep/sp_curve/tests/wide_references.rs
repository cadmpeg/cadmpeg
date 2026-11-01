// SPDX-License-Identifier: Apache-2.0

use crate::brep::sp_curve::scan;
use crate::test_support::parasolid::wide;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use std::collections::HashMap;

#[test]
fn wide_surface_curve_headers_preserve_the_support_and_pcurve() {
    for key in [0x7fff, 0x8000, 0xffff, 0x1_0000, 0x17f00] {
        for first in [false, true] {
            let surface = key + 1;
            let b_curve = key + 2;
            let body = wide::sp_curve(key, surface, b_curve, first);
            let pcurve = PcurveNurbs::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                None,
                false,
            )
            .unwrap();
            let curves = scan(&body, &HashMap::from([(b_curve, pcurve)]));
            assert_eq!(curves.len(), 1);
            let curve = curves.get(&key).expect("wide SP_CURVE");
            assert_eq!(curve.attr, key);
            assert_eq!(curve.surface, surface);
            assert_eq!(curve.pcurve.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
        }
    }
}
