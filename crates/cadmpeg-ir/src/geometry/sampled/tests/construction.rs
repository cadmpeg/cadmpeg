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
fn polygonal_construction_preserves_named_caller_refusals() {
    for operation in [
        "IR polygonal triangle index",
        "IR polygonal admitted vertices",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                crate::geometry::tests::budget::with_limit(
                    ResourceDimension::WorkUnits,
                    cap,
                    |ctx| PolygonalSurface::new(vertices(), vec![[0, 1, 2]], 0., ctx),
                )
            },
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
    // One triangle visit and three vertex yields plus collector exhaustion.
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "IR polygonal admitted vertices",
        |cap| {
            crate::geometry::tests::budget::with_policy(
                ResourceDimension::WorkUnits,
                cap,
                policy,
                |ctx| PolygonalSurface::new(vertices(), vec![[0, 1, 2]], 0.5, ctx),
            )
        },
    );
    policy.limits.max_work_units = 5;
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
    policy.limits.max_work_units = 1;
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

#[test]
fn polygonal_validation_charges_only_visited_triangles() {
    let admitted = || {
        vertices()
            .into_iter()
            .map(|point| FinitePoint3::new(point).expect("point"))
            .collect()
    };
    let construct = |ctx: &DecodeContext<'_>| {
        PolygonalSurface::from_admitted_scaled_deflection(
            admitted(),
            vec![[0, 1, 3], [0, 1, 2]],
            NonNegativeReal::new(0.).expect("deflection"),
            PositiveReal::new(1.).expect("scale"),
            ctx,
        )
    };
    let refusal = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "IR sampled construction refusal",
        |cap| {
            crate::geometry::tests::budget::with_limit(ResourceDimension::WorkUnits, cap, construct)
        },
    );
    assert!(matches!(refusal, CodecError::ResourceLimit(limit) if limit.used == 1));
    let message = "polygonal surface contains an out-of-range triangle index";
    // One visited triangle followed by the diagnostic's byte copy.
    let actual = crate::geometry::tests::budget::with_limit(
        ResourceDimension::WorkUnits,
        1 + cadmpeg_core::decode::u64_from_index(message.len()),
        construct,
    )
    .expect("one triangle and diagnostic");
    assert_eq!(actual, Err(GeometryLayoutError::Layout(message.into())));
}
