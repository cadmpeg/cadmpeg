// SPDX-License-Identifier: Apache-2.0

pub(super) fn with_test_context<T>(f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    f(&ctx)
}

pub(super) fn test_nurbs_curve(
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<cadmpeg_ir::math::Point3>,
    weights: Option<Vec<f64>>,
) -> cadmpeg_ir::geometry::nurbs::NurbsCurve {
    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        degree,
        knots,
        control_points,
        weights,
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid test NURBS curve")
}

// The fixture helper states each independent NURBS grid parameter explicitly.
#[allow(clippy::too_many_arguments)]
pub(super) fn test_nurbs_surface(
    u_degree: u32,
    v_degree: u32,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    _u_count: u32,
    v_count: u32,
    control_points: &[cadmpeg_ir::math::Point3],
    weights: Option<Vec<f64>>,
) -> cadmpeg_ir::geometry::nurbs::NurbsSurface {
    cadmpeg_ir::geometry::nurbs::NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(u_degree, u_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(v_degree, v_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
            control_points
                .chunks(cadmpeg_core::decode::index_from_u32(v_count))
                .map(<[_]>::to_vec)
                .collect(),
            weights.map(|values| {
                values
                    .chunks(cadmpeg_core::decode::index_from_u32(v_count))
                    .map(<[_]>::to_vec)
                    .collect()
            }),
        ),
        false,
    )
    .expect("fixture constructor admission")
    .expect("valid test NURBS surface")
}

