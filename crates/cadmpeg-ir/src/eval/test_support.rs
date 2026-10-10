// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// A degree-16 plane exercises admitted backing above inline spline support.
pub(in crate::eval) fn high_degree_plane() -> crate::geometry::nurbs::NurbsSurface {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::math::Point3;
    let axis = || {
        let mut knots = vec![0.0; 17];
        knots.extend([1.0; 17]);
        NurbsSurfaceAxis::new(16, knots, false)
    };
    let poles = (0..=16)
        .map(|u| {
            (0..=16)
                .map(|v| Point3::new(f64::from(u) / 16.0, f64::from(v) / 16.0, 0.0))
                .collect()
        })
        .collect();
    NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        axis(),
        axis(),
        NurbsSurfaceLanes::new(poles, None),
        false,
    )
    .expect("surface fixture admission")
    .expect("degree-16 plane")
}

pub(in crate::eval) fn with_policy<T>(
    policy: DecodePolicy,
    run: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(&ctx)
}
