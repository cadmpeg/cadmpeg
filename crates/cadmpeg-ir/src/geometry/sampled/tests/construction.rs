// SPDX-License-Identifier: Apache-2.0
use crate::features::FinitePoint3;
use crate::geometry::sampled::{GeometryLayoutError, PolygonalSurface};
use crate::math::Point3;
use crate::scalar::{NonNegativeReal, PositiveReal};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn vertices() -> Vec<Point3> {
    vec![
        Point3::new(0., 0., 0.),
        Point3::new(1., 0., 0.),
        Point3::new(0., 1., 0.),
    ]
}

#[test]
fn polygonal_construction_moves_typed_storage_and_admits_raw_conversion_once() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(3 * std::mem::size_of::<FinitePoint3>()).expect("bytes");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let raw = PolygonalSurface::new(vertices(), vec![[0, 1, 2]], 0.5).expect("valid");
    ctx.finish_session().expect("one conversion per vertex");
    assert_eq!(
        serde_json::from_str::<PolygonalSurface>(&serde_json::to_string(&raw).expect("wire"))
            .expect("context-free serde"),
        raw
    );

    let arena = DecodeArena::new();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let admitted = vertices()
        .into_iter()
        .map(|point| FinitePoint3::new(point).expect("finite"))
        .collect::<Vec<_>>();
    let triangles = vec![[0, 1, 2]];
    let vertex_address = admitted.as_ptr();
    let triangle_address = triangles.as_ptr();
    let typed = PolygonalSurface::from_admitted_scaled_deflection(
        admitted,
        triangles,
        NonNegativeReal::new(0.25).expect("deflection"),
        PositiveReal::new(2.).expect("scale"),
    )
    .expect("valid");
    assert_eq!(typed.vertices().as_ptr(), vertex_address);
    assert_eq!(typed.triangles().as_ptr(), triangle_address);
    assert_eq!(typed, raw);
    ctx.finish_session().expect("no conversion or copy");
}

#[test]
fn polygonal_construction_keeps_geometric_failure_order() {
    let mut points = vertices();
    points[0].x = f64::INFINITY;
    let error = PolygonalSurface::new(points.clone(), vec![[0, 1, 3]], -1.)
        .expect_err("bad index precedes coordinate and deflection");
    assert_eq!(
        error,
        GeometryLayoutError::Layout(
            "polygonal surface contains an out-of-range triangle index".into()
        )
    );
    let error = PolygonalSurface::new(points, vec![[0, 1, 2]], -1.)
        .expect_err("coordinate precedes deflection");
    assert_eq!(
        error,
        GeometryLayoutError::Layout("vertices must be finite".into())
    );
}
