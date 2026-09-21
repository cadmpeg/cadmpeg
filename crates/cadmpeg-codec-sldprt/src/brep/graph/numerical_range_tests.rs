// SPDX-License-Identifier: Apache-2.0

use super::*;

const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;
const INVERSE_FIT_TOLERANCE: f64 = 1e-6;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::math::Point2;

fn bilinear(domain: [f64; 2], scale: f64) -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![domain[0], domain[0], domain[1], domain[1]], false),
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0., 0., 0.), Point3::new(0., scale, 0.)],
                vec![Point3::new(scale, 0., 0.), Point3::new(scale, scale, 0.)],
            ],
            None,
        ),
        false,
    )
    .unwrap()
}
#[test]
fn numerical_0922_wide_domain_keeps_distinct_roots() {
    let r = unique_inverse_parameter(
        vec![(-5e307, 0.), (5e307, 0.)],
        INVERSE_FIT_TOLERANCE,
        [-1e308, 1e308],
    );
    println!("SW wide finite domain with two exact roots: {r:?}");
    assert!(matches!(r, InverseResolution::Ambiguous));
}
#[test]
fn numerical_0922_small_domain_keeps_fit_samples() {
    let mut s = bilinear([0., 1.], 1.);
    s.edit_control_points(|p| {
        p.z = p.x * p.y;
        Ok(())
    })
    .unwrap();
    for d in [1., SMALL_PARAMETER_DOMAIN] {
        let c = NurbsCurve::from_lanes(
            1,
            vec![0., 0., d, d],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 1., 1.)],
            None,
            false,
        )
        .unwrap();
        let samples = nurbs_curve_sample_parameters(&c, [0., d]).unwrap();
        let (uv, error) = nurbs_degree_one_cache_lanes(&s, &c, [0., d]).unwrap();
        let observed = Point3::new(0.5, 0.5, 0.5)
            .distance(cadmpeg_ir::eval::nurbs_surface_point(&s, 0.5, 0.5).unwrap());
        println!("SW d{d:e} samples{samples:?} uv{uv:?} reported_error={error} actual_midpoint_error={observed}");
        assert_eq!(error, observed);
        assert_eq!(samples.len(), 9);
        assert_eq!(samples.first(), Some(&0.0));
        assert_eq!(samples.last(), Some(&d));
    }
}
