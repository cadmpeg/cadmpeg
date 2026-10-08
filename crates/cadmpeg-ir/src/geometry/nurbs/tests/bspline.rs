// SPDX-License-Identifier: Apache-2.0

use crate::geometry::nurbs::{BsplineSurface, NurbsError};
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn points() -> Vec<Vec<Point3>> {
    vec![
        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        vec![Point3::new(3.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)],
    ]
}

#[test]
fn bspline_constructor_admits_each_scan_row_and_pole_before_its_visit() {
    for operation in [
        "IR NURBS knot finiteness",
        "IR NURBS knot order",
        "IR NURBS grid row shape",
        "IR admitted B-spline grid rows",
        "IR admitted B-spline grid poles",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                crate::geometry::tests::budget::with_limit(
                    ResourceDimension::WorkUnits,
                    cap,
                    |ctx| {
                        BsplineSurface::new(
                            ctx,
                            1,
                            1,
                            vec![0., 0., 1., 1.],
                            vec![0., 0., 1., 1.],
                            points(),
                        )
                    },
                )
            },
        );
    }
    // Each axis checks four finite knots and three pairs; one row width and nine collection probes.
    let cap = 24;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let surface = BsplineSurface::new(
        &ctx,
        1,
        1,
        vec![0., 0., 1., 1.],
        vec![0., 0., 1., 1.],
        points(),
    )
    .expect("every visit fits")
    .expect("valid surface");
    assert_eq!(
        surface
            .control_points
            .iter()
            .map(|row| row.iter().map(|point| point.get()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        points()
    );
    ctx.finish_session()
        .expect("no refusal after the last visit");
}

#[test]
fn bspline_constructor_preserves_storage_refusal_wire_and_source_order() {
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(limit)) = BsplineSurface::new(
            &ctx,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            points(),
        ) else {
            panic!("B-spline storage must refuse in the caller account");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, "IR admitted B-spline grid rows");
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two axes each check four finite knots and three pairs, then one row and nine collection probes.
    policy.limits.max_work_units = 24;
    policy.limits.max_collection_items = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let storage = knots.as_ptr();
    let surface = BsplineSurface::new(&ctx, 1, 1, knots, vec![0.0, 0.0, 1.0, 1.0], points())
        .expect("exact visits and slots")
        .expect("valid surface");
    assert_eq!(surface.u_knots.as_ptr(), storage);
    let wire = serde_json::json!({"u_degree":1,"v_degree":1,
        "u_knots":[0.0,0.0,1.0,1.0],"v_knots":[0.0,0.0,1.0,1.0],
        "control_points":[[ {"x":1.0,"y":0.0,"z":0.0}, {"x":2.0,"y":0.0,"z":0.0}],
                          [ {"x":3.0,"y":0.0,"z":0.0}, {"x":4.0,"y":0.0,"z":0.0}]]});
    assert_eq!(serde_json::to_value(&surface).expect("wire"), wire);
    assert_eq!(
        serde_json::from_value::<BsplineSurface>(wire).expect("context-free reconstruction"),
        surface
    );
    ctx.finish_session().expect("all admissions fit");
}

#[test]
fn bspline_constructor_preserves_axis_shape_and_scalar_error_precedence() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let mut invalid = points();
    invalid[1].pop();
    invalid[0][0].x = f64::NAN;
    assert_eq!(
        BsplineSurface::new(&ctx, 2, 1, knots.clone(), knots.clone(), invalid.clone())
            .expect("diagnostic admitted"),
        Err(NurbsError::Structure(
            "control_points u count must exceed degree 2, found 2".into()
        ))
    );
    assert_eq!(
        BsplineSurface::new(&ctx, 1, 1, knots.clone(), knots.clone(), invalid.clone())
            .expect("diagnostic admitted"),
        Err(NurbsError::Structure(
            "control_points row must contain 2 values, found 1".into()
        ))
    );
    invalid[1].push(Point3::new(4.0, 0.0, 0.0));
    assert_eq!(
        BsplineSurface::new(&ctx, 1, 1, knots.clone(), knots, invalid)
            .expect("diagnostic admitted"),
        Err(NurbsError::Structure(
            "control_points contains a non-finite point".into()
        ))
    );
}
