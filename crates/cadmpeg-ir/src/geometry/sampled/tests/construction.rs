// SPDX-License-Identifier: Apache-2.0
use crate::features::FinitePoint3;
use crate::geometry::sampled::{GeometryLayoutError, PolygonalSurface};
use crate::math::Point3;
use crate::scalar::{NonNegativeReal, PositiveReal};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn vertices() -> Vec<Point3> {
    vec![
        Point3::new(0., 0., 0.),
        Point3::new(1., 0., 0.),
        Point3::new(0., 1., 0.),
    ]
}

#[test]
fn polygonal_construction_preserves_first_and_later_caller_refusals() {
    for cap in 0..6 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) =
            PolygonalSurface::new(vertices(), vec![[0, 1, 2]], 0., &ctx)
        else {
            panic!("triangle comparisons and conversion visits require admission");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(
            limit.operation,
            if cap < 3 {
                "IR polygonal triangle index"
            } else {
                "IR polygonal admitted vertices"
            }
        );
        assert_eq!(limit.used, cap);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes =
                    u64::try_from(3 * std::mem::size_of::<FinitePoint3>() - 1).expect("bytes");
            }
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 2,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) =
            PolygonalSurface::new(vertices(), vec![[0, 1, 2]], 0., &ctx)
        else {
            panic!("admitted vertex output requires storage");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}

#[test]
fn polygonal_construction_moves_typed_storage_and_admits_raw_conversion_once() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(3 * std::mem::size_of::<FinitePoint3>()).expect("bytes");
    policy.limits.max_collection_items = 3;
    // Three triangle indexes plus three vertex yields and collector exhaustion cost 7.
    policy.limits.max_work_units = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(limit)) =
        PolygonalSurface::new(vertices(), vec![[0, 1, 2]], 0.5, &ctx)
    else {
        panic!("collector exhaustion must refuse at six work units");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 6);
    assert_eq!(limit.operation, "IR polygonal admitted vertices");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );

    let arena = DecodeArena::new();
    policy.limits.max_work_units = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let raw = PolygonalSurface::new(vertices(), vec![[0, 1, 2]], 0.5, &ctx)
        .expect("exact limits")
        .expect("valid");
    ctx.finish_session().expect("one conversion per vertex");
    assert_eq!(
        serde_json::from_str::<PolygonalSurface>(&serde_json::to_string(&raw).expect("wire"))
            .expect("context-free serde"),
        raw
    );

    let arena = DecodeArena::new();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_work_units = 3;
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
        &ctx,
    )
    .expect("layout only")
    .expect("valid");
    assert_eq!(typed.vertices().as_ptr(), vertex_address);
    assert_eq!(typed.triangles().as_ptr(), triangle_address);
    assert_eq!(typed, raw);
    ctx.finish_session().expect("no conversion or copy");
}

#[test]
fn polygonal_construction_admits_diagnostics_and_keeps_geometric_failure_order() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut points = vertices();
    points[0].x = f64::INFINITY;
    let error = PolygonalSurface::new(points.clone(), vec![[0, 1, 3]], -1., &ctx)
        .expect("admission")
        .expect_err("bad index precedes coordinate and deflection");
    assert_eq!(
        error,
        GeometryLayoutError::Layout(
            "polygonal surface contains an out-of-range triangle index".into()
        )
    );
    let error = PolygonalSurface::new(points, vec![[0, 1, 2]], -1., &ctx)
        .expect("admission")
        .expect_err("coordinate precedes deflection");
    assert_eq!(
        error,
        GeometryLayoutError::Layout("vertices must be finite".into())
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(limit)) =
        PolygonalSurface::new(Vec::new(), Vec::new(), -1., &ctx)
    else {
        panic!("layout diagnostic needs retained text admission");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "IR sampled construction refusal");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}
