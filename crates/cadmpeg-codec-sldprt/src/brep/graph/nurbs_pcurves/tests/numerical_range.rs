// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis;
use cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes;
use cadmpeg_ir::math::Point3;
use crate::brep::graph::nurbs_pcurves::nurbs_curve_sample_parameters;
use crate::brep::graph::nurbs_pcurves::nurbs_degree_one_cache_lanes;
const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;

fn bilinear(domain: [f64; 2], scale: f64) -> NurbsSurface {
    NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
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
    .expect("fixture constructor admission")
    .unwrap()
}
#[test]
fn numerical_0922_small_domain_keeps_fit_samples() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("test context");
    let mut s = bilinear([0., 1.], 1.);
    s.try_map_control_points(
        |_, p| {
            let mut p = p.get();
            p.z = p.x * p.y;
            cadmpeg_ir::features::FinitePoint3::new(p).ok_or_else(|| {
                cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".into(),
                )
            })
        },
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("pole edit admission")
    .unwrap();
    for d in [1., SMALL_PARAMETER_DOMAIN] {
        let c = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0., 0., d, d],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 1., 1.)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .unwrap();
        let samples = nurbs_curve_sample_parameters(&ctx, &c, [0., d])
            .unwrap()
            .unwrap();
        let (uv, error) = nurbs_degree_one_cache_lanes(&ctx, &s, &c, [0., d])
            .expect("resource allocation did not fail")
            .unwrap();
        let observed = Point3::new(0.5, 0.5, 0.5).distance(
            cadmpeg_ir::eval::decode::nurbs_surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &s,
                0.5,
                0.5,
            )
            .unwrap()
            .get(),
        );
        println!("SW d{d:e} samples{:?} uv{uv:?} reported_error={error} actual_midpoint_error={observed}", samples.values);
        assert_eq!(error, observed);
        assert_eq!(samples.values.len(), 9);
        assert_eq!(samples.values.first(), Some(&0.0));
        assert_eq!(samples.values.last(), Some(&d));
    }
}

