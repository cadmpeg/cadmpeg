// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::eval::{decode::Scratch, EvaluationFailure};
use crate::eval::surface_nurbs::nurbs_surface_local;
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::math::Point3;

#[test]
fn surface_derivatives_use_the_existing_scratch_context() {
    let source = cadmpeg_test_support::service_decode_context();
    let surface = NurbsSurface::from_lanes(
        &source,
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
    .expect("surface admission")
    .expect("quadratic surface");
    let source_scratch = Scratch::new(&source);
    let local =
        nurbs_surface_local(&source_scratch, &surface, 0.25, 0.75).expect("finite local surface");
    let first = local.first(&source_scratch).expect("finite first partials");
    assert_eq!(
        first
            .lanes
            .map(|row| row.map(crate::scalar::FiniteReal::get)),
        [[1.0, 0.0, 0.5], [0.0, 1.0, 1.5]]
    );
    assert_eq!(
        local
            .second(&source_scratch, &first)
            .expect("finite second partials")
            .lanes
            .map(|row| row.map(crate::scalar::FiniteReal::get)),
        [[0.0, 0.0, 2.0], [0.0, 0.0, 0.0], [0.0, 0.0, 2.0]]
    );
    for second in [false, true] {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!("tested derivative dimensions"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let scratch = Scratch::new(&ctx);
            let result = if second {
                local.second(&scratch, &first).map(|_| ())
            } else {
                local.first(&scratch).map(|_| ())
            };
            let Err(EvaluationFailure::ResourceLimit(limit)) = result else {
                panic!("caller refusal was not returned");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(scratch.refused(), Some(limit));
            drop(scratch);
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
    }
}
