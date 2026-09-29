// SPDX-License-Identifier: Apache-2.0

use crate::native::features::{
    feature_input_blocks, feature_operation_labels, feature_operation_records,
    feature_sketch_construction_inputs, feature_sketch_construction_payloads,
    feature_sketch_fixed_points, feature_sketch_payload_fixed_pairs,
    feature_sketch_payload_mixed_pairs, feature_sketch_payload_named_records,
    feature_sketch_payload_names, feature_sketch_payload_scalars, feature_sketch_points,
    feature_sketch_point_groups, feature_sketch_point_uses,
    feature_sketch_records, feature_sketch_references,
};

#[derive(Clone, Copy)]
enum SketchPointRoute { Floating, Fixed }

fn sketch_point_refusal(
    route: SketchPointRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx,
            crate::test_support::test_prt::composed_feature_history_prt())
    }).expect("composed feature-history container");
    let (records, names, scalars, pairs) = crate::test_support::with_decode_context(|ctx| {
        let labels = feature_operation_labels(ctx, &container)?;
        let operations = feature_operation_records(ctx, &container)?;
        let inputs = feature_input_blocks(ctx, &container)?;
        let references = feature_sketch_references(ctx, &container)?;
        let sketches = feature_sketch_records(ctx, &labels, &operations, &inputs, &references)?;
        let constructions = feature_sketch_construction_inputs(ctx, &sketches, &references)?;
        let payloads = feature_sketch_construction_payloads(ctx, &container, &constructions)?;
        let names = feature_sketch_payload_names(ctx, &container, &constructions)?;
        let scalars = feature_sketch_payload_scalars(ctx, &container, &constructions)?;
        let pairs = feature_sketch_payload_fixed_pairs(ctx, &container, &payloads)?;
        let mixed = feature_sketch_payload_mixed_pairs(ctx, &container, &payloads)?;
        let records = feature_sketch_payload_named_records(ctx, &payloads, &names,
            &scalars, &pairs, &mixed)?;
        Ok::<_, cadmpeg_core::CodecError>((records, names, scalars, pairs))
    }).expect("sketch point input records");
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        match route {
            SketchPointRoute::Floating =>
                feature_sketch_points(ctx, &records, &names, &scalars).map(|rows| rows.len()),
            SketchPointRoute::Fixed =>
                feature_sketch_fixed_points(ctx, &records, &names, &pairs).map(|rows| rows.len()),
        }
    };
    assert!(crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted sketch point route") > 0);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    decode(&ctx).expect_err("sketch point resource limit")
}

macro_rules! sketch_point_limit_tests {
    ($collection:ident, $retained:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = sketch_point_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = sketch_point_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $work() {
            let error = sketch_point_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

sketch_point_limit_tests!(
    sketch_point_refuses_collection_limit,
    sketch_point_refuses_retained_limit,
    sketch_point_refuses_work_limit,
    SketchPointRoute::Floating
);
sketch_point_limit_tests!(
    sketch_fixed_point_refuses_collection_limit,
    sketch_fixed_point_refuses_retained_limit,
    sketch_fixed_point_refuses_work_limit,
    SketchPointRoute::Fixed
);

fn sketch_point_use_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let point = crate::native::features::FeatureSketchPoint {
        id: "payload-point".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        named_record: "named-record".into(),
        name: "Point1".into(),
        coordinates: cadmpeg_ir::units::FiniteVector::new([1.0, 2.0])
            .expect("finite coordinates"),
        scalar_fields: ["scalar-1".into(), "scalar-2".into()],
    };
    let groups = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_point_groups(ctx, &[point])
    }).expect("sketch point group");
    let named_point = crate::native::features::OffsetStoreNamedPoint {
        id: "named-point".into(),
        name: "Point1".into(),
        data_blocks: vec!["block-10".into()],
        values: [(1.0, 200), (2.0, 220)].map(|(value, source_offset)| {
            crate::native::features::FeatureBinary64ScalarToken {
                scalar: crate::om::scalar::ShiftedBinary64::try_from(
                    crate::test_support::test_bytes::shifted_f64_bytes(value),
                ).expect("shifted scalar"),
                source_offset,
            }
        }),
        source_offset: 190,
    };
    let block_use = crate::native::features::FeatureSketchNamedPointBlockUse {
        id: "nx:feature-history:sketch-named-point-block-use#1-4-0".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        sketch_reference: "reference".into(),
        reference_ordinal: 0,
        named_point: named_point.id.clone(),
        data_block: "block-10".into(),
        point_block_ordinal: 0,
        source_offset: 300,
    };
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_sketch_point_uses(ctx, &groups,
            std::slice::from_ref(&named_point), std::slice::from_ref(&block_use))
    };
    assert_eq!(crate::test_support::with_decode_context(|ctx| decode(ctx))
        .expect("admitted point use").len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    decode(&ctx).expect_err("sketch point use resource limit")
}

#[test]
fn sketch_point_use_refuses_collection_limit() {
    let error = sketch_point_use_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn sketch_point_use_refuses_retained_limit() {
    let error = sketch_point_use_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn sketch_point_use_refuses_scoped_limit() {
    let error = sketch_point_use_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn sketch_point_use_refuses_work_limit() {
    let error = sketch_point_use_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
