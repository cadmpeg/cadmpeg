// SPDX-License-Identifier: Apache-2.0

use super::*;

const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;

#[test]
fn numerical_0922_interior_trim_is_preserved() {
    for domain in [1., SMALL_PARAMETER_DOMAIN] {
        let g = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::from_lanes(
                1,
                vec![0., 0., domain, domain],
                vec![Point2::new(0., 0.), Point2::new(10., 0.)],
                None,
                false,
            )
            .unwrap(),
        };
        let range = [0.1 * domain, 0.9 * domain];
        let result = normalize_pcurve_parameter_range(&g, Some(range)).unwrap();
        println!("FreeCAD domain {domain:e}, input {range:?} => {result:?}");
        assert_eq!(result, range);
    }
}
