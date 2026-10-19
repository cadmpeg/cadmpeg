// SPDX-License-Identifier: Apache-2.0
//! Coordinate scaling of decoded pcurve geometry.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles};
use crate::math::Point2;

fn context_with_limit<'a>(arena: &'a DecodeArena, policy: &'a DecodePolicy) -> DecodeContext<'a> {
    DecodeContext::from_root_bytes(b"", arena, policy)
        .expect("empty root fits policy")
        .0
}

#[test]
fn pcurve_coordinate_scale_scales_nurbs_controls() {
    let nurbs = PcurveNurbs::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        PcurveNurbsPoles::Polynomial {
            points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        },
        false,
    )
    .expect("valid pcurve NURBS");
    let mut geometry = PcurveGeometry::Nurbs { nurbs };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = context_with_limit(&arena, &policy);
    geometry = geometry
        .scaled_coordinates_owned(&ctx, [2.0, 3.0])
        .expect("work admitted")
        .expect("finite coordinates");
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        panic!("the pcurve form stays NURBS");
    };
    assert_eq!(
        nurbs.control_points(),
        vec![Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)]
    );
}
