// SPDX-License-Identifier: Apache-2.0

use super::{NurbsChart, Placement};
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::math::Point3;

fn bipyramid(gap: f64, cap_open: bool) -> NurbsSurface {
    let h = 3.0_f64.sqrt();
    let mut poles = vec![
        vec![Point3::new(0.0, 0.0, 3.0); 4],
        vec![
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(-1.0, h, 0.0),
            Point3::new(-1.0, -h, 0.0),
            Point3::new(2.0 + gap, 0.0, 0.0),
        ],
        vec![Point3::new(0.0, 0.0, -3.0); 4],
    ];
    if cap_open {
        poles[0][1].x = 1.0;
    }
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 2.0, 2.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0], false),
        NurbsSurfaceLanes::new(poles, None),
        false,
    )
    .unwrap()
}

#[test]
fn clamped_poles_and_a_matching_seam_close_the_domain() {
    let surface = bipyramid(0.0, false);
    let chart = NurbsChart::new(&surface, Placement::identity()).unwrap();
    assert!(chart.closed_domain(0.0));
}

#[test]
fn an_open_seam_or_an_open_cap_keeps_the_domain_open() {
    for surface in [bipyramid(0.125, false), bipyramid(0.0, true)] {
        let chart = NurbsChart::new(&surface, Placement::identity()).unwrap();
        assert!(!chart.closed_domain(1.0e-6));
    }
}
