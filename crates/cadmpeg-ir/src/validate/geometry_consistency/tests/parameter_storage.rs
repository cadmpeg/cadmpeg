// SPDX-License-Identifier: Apache-2.0

use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn parameter_uniqueness_preserves_first_values_and_negative_zero() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let values = super::super::unique(&ctx, &[3.0, f64::NAN, -0.0, 0.0, 3.0, 2.0], |value| {
        FiniteReal::new(*value)
    })
    .unwrap();
    assert_eq!(
        values.iter().map(|value| value.get()).collect::<Vec<_>>(),
        [3.0, -0.0, 2.0]
    );
    assert!(values[1].get().is_sign_negative());
}

#[test]
fn parameter_uniqueness_admits_source_before_projection_and_comparison_before_storage() {
    for cap in [0, 3] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let projected = std::cell::Cell::new(0);
        let Err(CodecError::ResourceLimit(limit)) =
            super::super::unique(&ctx, &[1.0, 2.0], |value| {
                projected.set(projected.get() + 1);
                FiniteReal::new(*value)
            })
        else {
            panic!("uniqueness must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(projected.get(), if cap == 0 { 0 } else { 2 });
        assert_eq!(
            limit.operation,
            if cap == 0 {
                "parameter uniqueness source scan"
            } else {
                "parameter uniqueness comparison"
            }
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn parameter_uniqueness_admits_only_visited_candidates() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The total is four source slots, three scratch copies, and four visited comparisons.
    policy.limits.max_work_units = 11;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let values =
        super::super::unique(&ctx, &[1.0, 2.0, 3.0, 1.0], |value| FiniteReal::new(*value)).unwrap();
    assert_eq!(
        values.iter().map(|value| value.get()).collect::<Vec<_>>(),
        [1.0, 2.0, 3.0]
    );
    drop(values);
    ctx.finish_session().unwrap();
}

#[test]
fn parameter_pairs_preserve_first_and_later_admission_refusals() {
    for (dimension, cap) in [
        (ResourceDimension::MaterializedBytes, 0),
        (ResourceDimension::CollectionItems, 0),
        (ResourceDimension::CollectionItems, 1),
        (ResourceDimension::WorkUnits, 0),
        (ResourceDimension::WorkUnits, 3),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = super::super::parameter_pairs(
            &ctx,
            &[FiniteReal::ONE],
            &[FiniteReal::new(3.0).unwrap(), FiniteReal::new(4.0).unwrap()],
        ) else {
            panic!("pair storage must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn declared_parameter_candidates_keep_order_and_release_scoped_storage() {
    let pcurve = crate::geometry::pcurve::Pcurve {
        id: "test:model:pcurve#ranges".try_into().unwrap(),
        geometry: crate::geometry::pcurve::PcurveGeometry::Line(
            crate::geometry::pcurve::LinePcurve::try_new(
                crate::math::Point2::new(0.0, 0.0),
                crate::math::Point2::new(1.0, 0.0),
            )
            .unwrap(),
        ),
        metadata: crate::geometry::pcurve::PcurveMetadata::default(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let ranges = super::super::pcurve_parameter_ranges(
            &ctx,
            &pcurve,
            Some([2.0, 3.0]),
            Some([0.0, 1.0]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(&ranges[..], [[2.0, 3.0], [0.0, 1.0], [-0.0, -1.0]]);
        assert!(ranges[2][0].is_sign_negative());
        let pairs = super::super::parameter_pairs(
            &ctx,
            &[FiniteReal::ONE],
            &[FiniteReal::new(3.0).unwrap(), FiniteReal::new(4.0).unwrap()],
        )
        .unwrap();
        assert_eq!(&pairs[..], [[1.0, 3.0], [1.0, 4.0]]);
    }
    drop(
        ctx.reserve_scoped(4096, "parameter candidate scopes released")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}
