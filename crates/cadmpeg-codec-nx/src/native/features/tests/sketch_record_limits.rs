// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_input_blocks;
use crate::native::features::feature_operation_labels;
use crate::native::features::feature_operation_records;
use crate::native::features::feature_sketch_construction_inputs;
use crate::native::features::feature_sketch_records;
use crate::native::features::feature_sketch_references;

#[derive(Clone, Copy)]
enum SketchRoute {
    Record,
    ConstructionInput,
}

fn sketch_record_refusal(
    route: SketchRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            crate::test_support::test_prt::composed_feature_history_prt(),
        )
    })
    .expect("composed feature-history container");
    let (labels, records, inputs, references) = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            feature_operation_labels(ctx, &container)?,
            feature_operation_records(ctx, &container)?,
            feature_input_blocks(ctx, &container)?,
            feature_sketch_references(ctx, &container)?,
        ))
    })
    .expect("sketch record inputs");
    let sketches = crate::test_support::with_decode_context(|ctx| {
        feature_sketch_records(ctx, &labels, &records, &inputs, &references)
    })
    .expect("admitted sketch records");
    assert!(!sketches.is_empty());
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route {
        SketchRoute::Record => feature_sketch_records(ctx, &labels, &records, &inputs, &references)
            .map(|rows| rows.len()),
        SketchRoute::ConstructionInput => {
            feature_sketch_construction_inputs(ctx, &sketches, &references).map(|rows| rows.len())
        }
    };
    assert!(
        crate::test_support::with_decode_context(|ctx| decode(ctx))
            .expect("admitted sketch record route")
            > 0
    );

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| decode(ctx).expect_err("sketch record resource limit"),
    )
}

macro_rules! sketch_record_limit_tests {
    ($collection:ident, $retained:ident, $scoped:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = sketch_record_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = sketch_record_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $scoped() {
            let error = sketch_record_refusal($route,
                |policy| policy.limits.max_materialized_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
        }
        #[test]
        fn $work() {
            let error = sketch_record_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

sketch_record_limit_tests!(
    sketch_record_refuses_collection_limit,
    sketch_record_refuses_retained_limit,
    sketch_record_refuses_scoped_limit,
    sketch_record_refuses_work_limit,
    SketchRoute::Record
);
sketch_record_limit_tests!(
    sketch_construction_input_refuses_collection_limit,
    sketch_construction_input_refuses_retained_limit,
    sketch_construction_input_refuses_scoped_limit,
    sketch_construction_input_refuses_work_limit,
    SketchRoute::ConstructionInput
);
