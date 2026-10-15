// SPDX-License-Identifier: Apache-2.0
use crate::eval::{decode::Scratch, nurbs_surface_local};
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::math::Point3;

#[test]
fn quadratic_surface_local_partials_match_the_closed_form() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..3)
                .map(|i| {
                    (0..3)
                        .map(|j| {
                            Point3::new(
                                f64::from(i) / 2.0,
                                f64::from(j) / 2.0,
                                f64::from(u8::from(i == 2)) + f64::from(u8::from(j == 2)),
                            )
                        })
                        .collect()
                })
                .collect(),
            None,
        ),
        false,
    )
    .expect("quadratic surface");
    let scratch = Scratch::new(&ctx);
    let local = nurbs_surface_local(&scratch, &surface, 0.25, 0.75).expect("finite local surface");
    let first = local.first(&scratch).expect("finite first partials");
    assert_eq!(
        first
            .lanes
            .map(|row| row.map(crate::scalar::FiniteReal::get)),
        [[1.0, 0.0, 0.5], [0.0, 1.0, 1.5]]
    );
    assert_eq!(
        local
            .second(&scratch, &first)
            .expect("finite second partials")
            .map(|row| row.map(crate::scalar::FiniteReal::get)),
        [[0.0, 0.0, 2.0], [0.0, 0.0, 0.0], [0.0, 0.0, 2.0]]
    );
}
