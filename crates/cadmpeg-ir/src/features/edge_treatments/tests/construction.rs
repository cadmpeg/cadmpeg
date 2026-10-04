// SPDX-License-Identifier: Apache-2.0
use super::point;
use crate::features::edge_treatments::{VariableRadii, VariableRadius};
use crate::scalar::{Fraction, NonNegativeLength};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn raw() -> Vec<VariableRadius> {
    vec![point(0., 0.), point(0.5, 0.), point(1., 2.)]
}
fn typed() -> Vec<VariableRadius<Fraction, NonNegativeLength>> {
    raw()
        .into_iter()
        .map(|point| VariableRadius {
            parameter: Fraction::new(point.parameter).expect("fraction"),
            radius: NonNegativeLength::try_from(point.radius).expect("nonnegative"),
        })
        .collect()
}

#[test]
fn radius_construction_admits_conversion_positivity_and_order() {
    for raw_input in [false, true] {
        // Raw collection charges one probe per sample and one terminal probe.
        let offset = if raw_input { 4 } else { 0 };
        for cap in 0..offset + 5 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = if raw_input {
                VariableRadii::new(raw(), &ctx)
            } else {
                VariableRadii::from_parts(typed(), &ctx)
            };
            let Err(CodecError::ResourceLimit(limit)) = result else {
                panic!("each visit requires admission");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.used, cap);
            assert_eq!(
                limit.operation,
                if cap < offset {
                    "IR variable radius admitted samples"
                } else if cap < offset + 3 {
                    "IR variable radius positivity"
                } else {
                    "IR variable radius parameter comparison"
                }
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        }
    }
    let bytes =
        u64::try_from(3 * std::mem::size_of::<VariableRadius<Fraction, NonNegativeLength>>())
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
        let Err(CodecError::ResourceLimit(limit)) = VariableRadii::new(raw(), &ctx) else {
            panic!("final output needs retained storage and slots");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
}

#[test]
fn radius_construction_moves_typed_storage_and_shares_serde_admission() {
    let bytes =
        u64::try_from(3 * std::mem::size_of::<VariableRadius<Fraction, NonNegativeLength>>())
            .expect("bytes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Four collector probes (three samples and terminal None), three
    // positivity visits through the last positive radius, and two order
    // comparisons admit the raw path.
    policy.limits.max_work_units = 9;
    policy.limits.max_retained_bytes = bytes;
    policy.limits.max_collection_items = 3;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let law = VariableRadii::new(raw(), &ctx)
        .expect("one allocation and exact visits")
        .expect("valid");
    ctx.finish_session().expect("no scratch or duplicate scan");
    assert_eq!(
        serde_json::from_str::<VariableRadii>(&serde_json::to_string(&law).expect("wire"))
            .expect("context-free serde"),
        law
    );
    let arena = DecodeArena::new();
    // Typed rows move directly into the result; only three positivity visits
    // and two adjacent parameter comparisons consume work.
    policy.limits.max_work_units = 5;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let points = typed();
    let address = points.as_ptr();
    let checked = VariableRadii::from_parts(points, &ctx)
        .expect("scan only")
        .expect("valid");
    assert_eq!(checked.as_slice().as_ptr(), address);
    assert_eq!(checked, law);
    ctx.finish_session().expect("source rows moved");
}

#[test]
fn radius_construction_keeps_geometry_absence_distinct_from_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let Err(CodecError::ResourceLimit(limit)) = VariableRadii::new(Vec::new(), &ctx) else {
        panic!("the empty raw collector needs its terminal probe");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    assert_eq!(limit.operation, "IR variable radius admitted samples");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The empty raw collector still charges the terminal `None` probe before
    // returning the static geometry diagnostic.
    policy.limits.max_work_units = 1;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        VariableRadii::new(Vec::new(), &ctx).expect("empty collector terminal probe"),
        Err(super::super::INVALID_VARIABLE_RADII)
    );
    assert_eq!(
        VariableRadii::from_parts(typed()[..1].to_vec(), &ctx).expect("fixed shape only"),
        Err(super::super::INVALID_VARIABLE_RADII)
    );
    ctx.finish_session()
        .expect("static diagnostic needs no allocation");
    let arena = DecodeArena::new();
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut points = typed();
    for point in &mut points {
        point.radius = NonNegativeLength::ZERO;
    }
    assert_eq!(
        VariableRadii::from_parts(points, &ctx).expect("three positivity visits"),
        Err(super::super::INVALID_VARIABLE_RADII)
    );
    ctx.finish_session()
        .expect("parameter order is not scanned after zero radii");
}
