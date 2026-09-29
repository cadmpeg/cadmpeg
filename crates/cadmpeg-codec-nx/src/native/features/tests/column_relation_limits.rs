// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_datum_csys_column_row_uses;
use crate::native::features::feature_input_column_row_uses;
use crate::native::features::feature_input_column_targets;
use crate::native::features::ColumnIndexRowKind;
use crate::native::features::FeatureDatumCsysConstruction;
use crate::native::features::FeatureInputBlock;
use crate::native::features::FeatureInputColumnRowUse;
use crate::native::om::column_row::DataBlockTargetIndexRow;
use crate::om::column_row::TargetRow;
use crate::om::compact::CompactIndexTarget;

#[derive(Clone, Copy)]
enum ColumnRoute {
    InputUse,
    DatumUse,
    Target,
}

fn column_relation_refusal(
    route_kind: ColumnRoute,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let input = FeatureInputBlock {
        id: "input#0".into(),
        operation_label: "operation#0".into(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(7, &[7])
            .expect("input object token"),
        data_block: "block#5".into(),
        source_offset: 10,
    };
    let construction = FeatureDatumCsysConstruction {
        id: "construction#0".into(),
        operation_label: "operation#0".into(),
        frame: crate::om::datum_csys::DatumCsysFrame::new(
            0x16,
            181,
            std::array::from_fn(|slot| {
                (
                    crate::om::reference_index::PayloadIndexToken::from_wire(
                        slot as u32,
                        &[0xf0, slot as u8],
                    )
                    .expect("datum reference token"),
                    format!("block#{slot}"),
                )
            }),
        )
        .expect("datum CSYS frame"),
    };
    let target = |value| CompactIndexTarget {
        atom: crate::om::compact::CompactIndexAtom::read(&[value]).expect("column target token"),
        target: format!("block#{value}"),
    };
    let row = DataBlockTargetIndexRow {
        id: "target-row#0".into(),
        section_ordinal: 0,
        ordinal: 0,
        frame: TargetRow::<String, u64>::new(
            target(5),
            [6, 7, 8].map(target),
            crate::om::discriminators::IndexRowMode::Form07,
            100,
        )
        .expect("target row"),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };
    let target_use = FeatureInputColumnRowUse {
        id: "input-column-use#0".into(),
        input_block: input.id.clone(),
        operation_label: input.operation_label.clone(),
        input_slot: input.input_slot,
        row_kind: ColumnIndexRowKind::TargetIndex,
        column_row: row.id.clone(),
        column_table: Some("table#0".into()),
        row_slot: crate::om::column_row::ColumnRowSlot::Zero,
        data_block: input.data_block.clone(),
        source_offset: 100,
    };
    let route = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| match route_kind {
        ColumnRoute::DatumUse => feature_datum_csys_column_row_uses(
            ctx,
            std::slice::from_ref(&construction),
            &[],
            &[],
            std::slice::from_ref(&row),
            &[],
        )
        .map(|uses| uses.len()),
        ColumnRoute::InputUse => feature_input_column_row_uses(
            ctx,
            std::slice::from_ref(&input),
            &[],
            &[],
            std::slice::from_ref(&row),
            &[],
        )
        .map(|uses| uses.len()),
        ColumnRoute::Target => feature_input_column_targets(
            ctx,
            std::slice::from_ref(&input),
            std::slice::from_ref(&target_use),
            &[],
            std::slice::from_ref(&row),
        )
        .map(|targets| targets.len()),
    };
    let admitted = crate::test_support::with_decode_context(|ctx| route(ctx))
        .expect("admitted column relation");
    assert_eq!(
        admitted,
        if matches!(route_kind, ColumnRoute::DatumUse) {
            3
        } else {
            1
        }
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    route(&ctx).expect_err("column relation resource limit")
}

macro_rules! column_relation_limit_tests {
    ($collection:ident, $retained:ident, $work:ident, $route:expr) => {
        #[test]
        fn $collection() {
            let error = column_relation_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }
        #[test]
        fn $retained() {
            let error = column_relation_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }
        #[test]
        fn $work() {
            let error = column_relation_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

column_relation_limit_tests!(
    input_column_join_refuses_collection_limit,
    input_column_join_refuses_retained_limit,
    input_column_join_refuses_work_limit,
    ColumnRoute::InputUse
);
column_relation_limit_tests!(
    datum_column_join_refuses_collection_limit,
    datum_column_join_refuses_retained_limit,
    datum_column_join_refuses_work_limit,
    ColumnRoute::DatumUse
);
column_relation_limit_tests!(
    input_column_target_refuses_collection_limit,
    input_column_target_refuses_retained_limit,
    input_column_target_refuses_work_limit,
    ColumnRoute::Target
);
