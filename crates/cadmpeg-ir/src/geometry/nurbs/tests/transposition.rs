// SPDX-License-Identifier: Apache-2.0
use super::super::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes, WeightedPole3};
use crate::features::FinitePoint3;
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn source(rational: bool) -> NurbsSurface {
    let points = (0..3).map(|u| (0..2).map(|v| Point3::new(f64::from(u), f64::from(v), 2.)).collect()).collect();
    let weights = rational.then(|| vec![vec![1., -2.], vec![3., -4.], vec![5., -6.]]);
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(2, vec![0., 0., 0., 1., 1., 1.], true),
        NurbsSurfaceAxis::new(1, vec![2., 2., 5., 5.], false),
        NurbsSurfaceLanes::new(points, weights), true,
    ).expect("construction admission").expect("valid surface")
}

#[test]
fn surface_transposition_refuses_before_mutation_at_every_copy_boundary() {
    for rational in [false, true] {
        let original = source(rational);
        for cap in 0..8 {
            let mut surface = original.clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let Err(CodecError::ResourceLimit(limit)) = surface.transpose_parameter_axes(&ctx) else {
                panic!("each column and pole copy requires admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.used, cap);
            assert_eq!(limit.additional, 1);
            assert_eq!(surface, original);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
    }
}

#[test]
fn surface_transposition_admits_exact_output_storage_and_preserves_axes() {
    for rational in [false, true] {
        let original = source(rational);
        let pole_bytes = if rational { std::mem::size_of::<WeightedPole3<FinitePoint3>>() } else { std::mem::size_of::<FinitePoint3>() };
        let bytes = u64::try_from(2 * std::mem::size_of::<Vec<FinitePoint3>>() + 6 * pole_bytes).expect("size");
        for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::CollectionItems] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = bytes - 1,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 7,
                _ => panic!("test dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut surface = original.clone();
            let Err(CodecError::ResourceLimit(limit)) = surface.transpose_parameter_axes(&ctx) else {
                panic!("all output storage requires admission");
            };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(surface, original);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = bytes;
        policy.limits.max_collection_items = 8;
        policy.limits.max_work_units = 8;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut surface = original.clone();
        surface.transpose_parameter_axes(&ctx).expect("exact output limit");
        assert_eq!((surface.u_degree(), surface.v_degree()), (1, 2));
        assert_eq!((surface.u_count(), surface.v_count()), (2, 3));
        assert_eq!(surface.u_knots(), original.v_knots());
        assert_eq!(surface.v_knots(), original.u_knots());
        assert_eq!((surface.u_periodic(), surface.v_periodic()), (false, true));
        assert!(surface.normal_reversed());
        for u in 0..3 {
            for v in 0..2 {
                assert_eq!(surface.pole(v, u), original.pole(u, v));
                assert_eq!(surface.weight(v, u), original.weight(u, v));
            }
        }
        ctx.finish_session().expect("one output allocation and copy per item");
    }
}
