// SPDX-License-Identifier: Apache-2.0
use crate::features::FinitePoint3;
use crate::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
use crate::math::Point3;
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn raw(parameters: Option<[f64; 3]>) -> PolylineSamples {
    let points = [
        Point3::new(0., 0., 0.),
        Point3::new(1., 0., 0.),
        Point3::new(1., 1., 0.),
    ];
    match parameters {
        None => PolylineSamples::Unparameterized {
            points: points.to_vec().try_into().expect("three points"),
        },
        Some(parameters) => PolylineSamples::Parameterized {
            vertices: points
                .into_iter()
                .zip(parameters)
                .map(|(point, parameter)| PolylineVertex { parameter, point })
                .collect::<Vec<_>>()
                .try_into()
                .expect("three vertices"),
        },
    }
}

fn checked(parameters: Option<[f64; 3]>) -> PolylineSamples<FiniteReal, FinitePoint3> {
    match raw(parameters) {
        PolylineSamples::Unparameterized { points } => PolylineSamples::Unparameterized {
            points: points.map(|point| FinitePoint3::new(point).expect("finite")),
        },
        PolylineSamples::Parameterized { vertices } => PolylineSamples::Parameterized {
            vertices: vertices.map(|vertex| PolylineVertex {
                parameter: FiniteReal::new(vertex.parameter).expect("finite"),
                point: FinitePoint3::new(vertex.point).expect("finite"),
            }),
        },
    }
}

#[test]
fn polyline_construction_admits_every_raw_visit_and_final_storage_once() {
    // Three point checks + three sample yields + iterator exhaustion + 0/2/3 comparisons = 7/9/10.
    for (parameters, work) in [(None, 7), (Some([0., 1., 2.]), 9), (Some([2., 1., 0.]), 10)] {
        for cap in 0..work {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let Err(CodecError::ResourceLimit(limit)) =
                PolylineCurve::new(raw(parameters), 0.25, &ctx)
            else {
                panic!("each input visit requires admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.used, cap);
            assert_eq!(
                limit.operation,
                if cap < 3 {
                    "IR polyline point finiteness"
                } else if cap < 7 {
                    "IR polyline admitted samples"
                } else if parameters == Some([2., 1., 0.]) && cap > 7 {
                    "IR polyline decreasing parameter comparison"
                } else {
                    "IR polyline increasing parameter comparison"
                }
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let bytes = u64::try_from(
            3 * if parameters.is_some() {
                std::mem::size_of::<PolylineVertex<FiniteReal, FinitePoint3>>()
            } else {
                std::mem::size_of::<FinitePoint3>()
            },
        )
        .expect("bytes");
        for dimension in [
            ResourceDimension::RetainedBytes,
            ResourceDimension::CollectionItems,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = bytes - 1,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 2,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let Err(CodecError::ResourceLimit(limit)) =
                PolylineCurve::new(raw(parameters), 0.25, &ctx)
            else {
                panic!("final sample storage requires admission");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.operation, "IR polyline admitted samples");
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = bytes;
        policy.limits.max_collection_items = 3;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let curve = PolylineCurve::new(raw(parameters), 0.25, &ctx)
            .expect("exact admission")
            .expect("valid");
        ctx.finish_session()
            .expect("one final allocation and no scratch copies");
        assert_eq!(
            serde_json::from_str::<PolylineCurve>(&serde_json::to_string(&curve).expect("wire"))
                .expect("context-free serde"),
            curve
        );
    }
}

#[test]
fn polyline_construction_moves_checked_rows_and_admits_comparisons() {
    for (parameters, work) in [(None, 0), (Some([0., 1., 2.]), 2), (Some([2., 1., 0.]), 3)] {
        for cap in 0..work {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let Err(CodecError::ResourceLimit(limit)) =
                PolylineCurve::from_checked_samples(checked(parameters), 0.25, &ctx)
            else {
                panic!("comparisons require caller admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.used, cap);
            assert_eq!(
                limit.operation,
                if parameters == Some([2., 1., 0.]) && cap > 0 {
                    "IR polyline decreasing parameter comparison"
                } else {
                    "IR polyline increasing parameter comparison"
                }
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let samples = checked(parameters);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let curve = match samples {
            PolylineSamples::Unparameterized { points } => {
                let address = points.as_ptr();
                let curve = PolylineCurve::from_checked_samples(
                    PolylineSamples::Unparameterized { points },
                    0.25,
                    &ctx,
                )
                .expect("only comparisons")
                .expect("valid");
                let PolylineSamples::Unparameterized { points } = &curve.samples else {
                    panic!("same sample lane");
                };
                assert_eq!(points.as_ptr(), address);
                curve
            }
            PolylineSamples::Parameterized { vertices } => {
                let address = vertices.as_ptr();
                let curve = PolylineCurve::from_checked_samples(
                    PolylineSamples::Parameterized { vertices },
                    0.25,
                    &ctx,
                )
                .expect("only comparisons")
                .expect("valid");
                let PolylineSamples::Parameterized { vertices } = &curve.samples else {
                    panic!("same sample lane");
                };
                assert_eq!(vertices.as_ptr(), address);
                curve
            }
        };
        assert_eq!(curve.chordal_deflection().get(), 0.25);
        ctx.finish_session().expect("no sample allocation or copy");
    }
}

#[test]
fn polyline_construction_preserves_diagnostic_order_and_refusal() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut samples = raw(Some([f64::INFINITY, 0., 0.]));
    let PolylineSamples::Parameterized { vertices } = &mut samples else {
        panic!("parameterized fixture");
    };
    vertices[0].point.x = f64::INFINITY;
    assert_eq!(
        PolylineCurve::new(samples, -1., &ctx)
            .expect("admission")
            .expect_err("points precede deflection")
            .to_string(),
        "points must be finite"
    );
    assert_eq!(
        PolylineCurve::new(raw(Some([f64::INFINITY, 0., 0.])), -1., &ctx)
            .expect("admission")
            .expect_err("deflection precedes parameters")
            .to_string(),
        "chordal_deflection must be finite and non-negative"
    );
    assert_eq!(
        PolylineCurve::new(raw(Some([f64::INFINITY, 0., 0.])), 0., &ctx)
            .expect("admission")
            .expect_err("nonfinite parameter")
            .to_string(),
        "parameters must be finite and strictly monotonic"
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units =
        u64::try_from("chordal_deflection must be finite and non-negative".len())
            .expect("diagnostic copy work");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        PolylineCurve::from_checked_samples(checked(Some([0., 0., 0.])), -1., &ctx)
            .expect("no parameter scan before deflection")
            .expect_err("negative deflection")
            .to_string(),
        "chordal_deflection must be finite and non-negative"
    );
    ctx.finish_session()
        .expect("only the diagnostic text is copied");
    let arena = DecodeArena::new();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let samples = PolylineSamples::Unparameterized {
        points: vec![Point3::new(f64::INFINITY, 0., 0.)]
            .try_into()
            .expect("nonempty"),
    };
    let Err(CodecError::ResourceLimit(limit)) = PolylineCurve::new(samples, -1., &ctx) else {
        panic!("count diagnostic needs retained admission before any sample visit");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "IR sampled construction refusal");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}
