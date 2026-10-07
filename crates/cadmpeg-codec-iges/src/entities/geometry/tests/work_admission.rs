// SPDX-License-Identifier: Apache-2.0

fn assert_scan_work_refusal<T>(
    operation: &str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let ctx = cadmpeg_core::decode::DecodeContext::new(&arena, &policy, false);
            run(&ctx)
        },
    );
}

#[test]
fn transform_preflight_refuses_work_before_chain_step() {
    let directory = [super::transform_entry(1, 0), super::transform_entry(3, 1)];
    assert_scan_work_refusal("iges transform preflight walk", |ctx| {
        super::super::enforce_transform_depth(&directory, ctx)
    });
}

#[test]
fn consumed_support_closure_refuses_work_before_advance() {
    let directory = [super::transform_entry(1, 0), super::transform_entry(3, 1)];
    let records = std::collections::BTreeMap::new();
    assert_scan_work_refusal("iges consumed-support closure traversal", |ctx| {
        super::super::consumed_support_sequences(&directory, &records, ctx).map(|_| ())
    });
}

#[test]
fn geometry_driver_admits_directory_and_family_passes() {
    use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
    let bytes = crate::test_support::test_curves_and_surfaces::line_file(0);
    for operation in [
        "iges geometry directory admission",
        "iges geometry parameter traversal",
        "iges geometry directory index traversal",
        "iges geometry family traversal",
        "iges composite directory traversal",
        "iges conic directory traversal",
        "iges analytic vertex traversal",
        "iges analytic point retention",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::IgesCodec
                    .decode(
                        &mut std::io::Cursor::new(&bytes),
                        &DecodeOptions {
                            policy,
                            ..DecodeOptions::default()
                        },
                    )
                    .map_err(|failure| match failure {
                        DecodeFailure::Codec(error) => error,
                        other => panic!("unexpected decode failure: {other:?}"),
                    })
            },
        );
    }
}

#[test]
fn linear_nurbs_parameter_knots_admit_work_before_materialization() {
    let knots = [0.0, 0.0, 0.5, 1.0, 1.0];
    assert_scan_work_refusal("iges linear NURBS parameter knots", |ctx| {
        super::super::linear_nurbs_parameters(1, &knots, 3, false, [0.0, 1.0], ctx).map(|_| ())
    });
    crate::test_support::with_service_context(&[], |ctx| {
        let (parameters, _storage) =
            super::super::linear_nurbs_parameters(1, &knots, 3, false, [0.0, 1.0], ctx)
                .unwrap()
                .unwrap();
        assert_eq!(parameters, vec![0.0, 0.5, 1.0]);
    });
}

#[test]
fn nurbs_plane_searches_admit_the_visited_controls() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(0.0, 0.0, 1.0),
    ]
    .map(|point| FinitePoint3::new(point).unwrap());
    for operation in [
        "iges NURBS plane direction search",
        "iges NURBS plane normal search",
        "iges NURBS planarity check",
    ] {
        assert_scan_work_refusal(operation, |ctx| {
            super::super::classify_control_point_plane(&points, 0.0, ctx)
        });
    }
    crate::test_support::with_service_context(&[], |ctx| {
        assert!(matches!(
            super::super::classify_control_point_plane(&points, 0.0, ctx).unwrap(),
            super::super::ControlPointPlane::NonPlanar
        ));
        assert!(matches!(
            super::super::classify_control_point_plane(&points[..3], 0.0, ctx).unwrap(),
            super::super::ControlPointPlane::Unique
        ));
    });
}

#[test]
fn declared_control_intervals_use_the_callers_scratch_reservation() {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let values = [
        126, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 1,
    ];
    let record = ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        values.len(),
        values
            .into_iter()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        Vec::new(),
    );
    let precision = crate::global::RealPrecision {
        single_significance: 6,
        double_significance: 15,
    };
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges Type126 declared control intervals",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let arena = DecodeArena::new();
            let ctx = DecodeContext::new(&arena, &policy, false);
            let mut storage = ctx.reserve_scoped(0, "test source control storage")?;
            super::super::type126_declared_control_points(&record, precision, &ctx, &mut storage)
        },
    );
    crate::test_support::with_service_context(&[], |ctx| {
        let mut storage = ctx
            .reserve_scoped(0, "test source control storage")
            .unwrap();
        let controls =
            super::super::type126_declared_control_points(&record, precision, ctx, &mut storage)
                .unwrap()
                .unwrap();
        assert_eq!(controls.len(), 2);
        assert!(controls[0][0].contains(0.0));
        assert!(controls[1][0].contains(2.0));
    });
}
