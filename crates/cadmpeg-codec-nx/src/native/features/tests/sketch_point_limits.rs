// SPDX-License-Identifier: Apache-2.0

use crate::native::features::{
    feature_input_blocks, feature_operation_labels, feature_operation_records,
    feature_sketch_construction_inputs, feature_sketch_construction_payloads,
    feature_sketch_fixed_points, feature_sketch_payload_fixed_pairs,
    feature_sketch_payload_mixed_pairs, feature_sketch_payload_named_records,
    feature_sketch_payload_names, feature_sketch_payload_scalars, feature_sketch_points,
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
