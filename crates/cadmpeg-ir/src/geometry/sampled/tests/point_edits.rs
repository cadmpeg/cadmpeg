// SPDX-License-Identifier: Apache-2.0
use crate::features::FinitePoint3;
use crate::geometry::sampled::{PolylineSamples, PolylineVertex};
use crate::math::Point3;
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn samples(parameterized: bool) -> PolylineSamples<FiniteReal, FinitePoint3> {
    let points = [
        Point3::new(0., 0., 0.),
        Point3::new(1., 0., 0.),
        Point3::new(1., 1., 0.),
    ]
    .map(|point| FinitePoint3::new(point).expect("finite"));
    if parameterized {
        PolylineSamples::Parameterized {
            vertices: points
                .into_iter()
                .zip([2., 1., 0.])
                .map(|(point, parameter)| PolylineVertex {
                    point,
                    parameter: FiniteReal::new(parameter).expect("finite"),
                })
                .collect::<Vec<_>>()
                .try_into()
                .expect("three rows"),
        }
    } else {
        PolylineSamples::Unparameterized {
            points: points.to_vec().try_into().expect("three points"),
        }
    }
}

#[test]
fn sampled_point_edit_refuses_before_copy_callback_and_copy_back() {
    for parameterized in [false, true] {
        let original = samples(parameterized);
        for cap in 0..9 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut edited = original.clone();
            let mut calls = 0;
            let Err(CodecError::ResourceLimit(limit)) = edited.edit_admitted_points(
                |point| {
                    calls += 1;
                    Ok::<_, ()>(point.negated())
                },
                &ctx,
            ) else {
                panic!("all transaction operations require admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(
                limit.operation,
                if cap < 3 {
                    "IR sampled edit candidate"
                } else if cap < 6 {
                    "IR sampled edit callback"
                } else {
                    "IR sampled edit copy back"
                }
            );
            assert_eq!(
                calls,
                if cap < 3 {
                    0
                } else if cap < 6 {
                    cap - 3
                } else {
                    3
                }
            );
            assert_eq!(edited, original);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
        let bytes = u64::try_from(
            3 * if parameterized {
                std::mem::size_of::<PolylineVertex<FiniteReal, FinitePoint3>>()
            } else {
                std::mem::size_of::<FinitePoint3>()
            },
        )
        .expect("bytes");
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = bytes - 1;
                }
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 2,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut edited = original.clone();
            let mut called = false;
            let Err(CodecError::ResourceLimit(limit)) = edited.edit_admitted_points(
                |point| {
                    called = true;
                    Ok::<_, ()>(point)
                },
                &ctx,
            ) else {
                panic!("scratch must be reserved first");
            };
            assert!(!called);
            assert_eq!(limit.dimension, dimension);
            assert_eq!(edited, original);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
    }
}

#[test]
fn sampled_point_edit_keeps_storage_parameters_and_releases_scratch() {
    for parameterized in [false, true] {
        let original = samples(parameterized);
        let bytes = u64::try_from(
            3 * if parameterized {
                std::mem::size_of::<PolylineVertex<FiniteReal, FinitePoint3>>()
            } else {
                std::mem::size_of::<FinitePoint3>()
            },
        )
        .expect("bytes");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 24;
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 9;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut edited = original.clone();
        let mut calls = 0;
        assert_eq!(
            edited
                .edit_admitted_points(
                    |point| {
                        calls += 1;
                        if calls == 3 {
                            Err("caller refused third point")
                        } else {
                            Ok(point.negated())
                        }
                    },
                    &ctx
                )
                .expect("scoped admission"),
            Err("caller refused third point")
        );
        assert_eq!(edited, original);
        let address = match &edited {
            PolylineSamples::Unparameterized { points } => points.as_ptr().cast::<()>(),
            PolylineSamples::Parameterized { vertices } => vertices.as_ptr().cast::<()>(),
        };
        for _ in 0..2 {
            edited
                .edit_admitted_points(|point| Ok::<_, ()>(point.negated()), &ctx)
                .expect("scratch reused")
                .expect("valid edit");
        }
        assert_eq!(edited, original);
        assert_eq!(
            match &edited {
                PolylineSamples::Unparameterized { points } => points.as_ptr().cast::<()>(),
                PolylineSamples::Parameterized { vertices } => vertices.as_ptr().cast::<()>(),
            },
            address
        );
        ctx.finish_session()
            .expect("scoped bytes are released on refusal and success");
    }
}
