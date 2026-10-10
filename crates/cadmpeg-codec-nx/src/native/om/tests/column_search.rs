// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn column_index_search_refusal(operation: &str) {
    use crate::native::om::column_row::{DataBlockLinkedIndexRow, DataBlockTargetIndexRow};
    use crate::om::column_row::{LinkedRow, TargetRow};
    use crate::om::compact::{CompactIndexAtom, CompactIndexTarget};
    use crate::om::discriminators::{IndexRowMode, LinkedIndexDiscriminator, LinkedIndexFlag};

    let atom = |value: u32| {
        CompactIndexAtom::from_wire(
            value,
            &[u8::try_from(value).expect("fixture value fits u8")],
        )
        .unwrap()
    };
    let target = |value| CompactIndexTarget {
        atom: atom(value),
        target: format!("block#{value}"),
    };
    let linked = |id: &str, value, mode, offset| DataBlockLinkedIndexRow {
        id: id.into(),
        section_ordinal: 0,
        ordinal: 0,
        frame: LinkedRow::<String, u64>::new(
            atom(20),
            LinkedIndexDiscriminator::Form16,
            target(value),
            [5, 6, 7].map(target),
            LinkedIndexFlag::Form03,
            mode,
            offset,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening".into(),
        opening_block_offset: 0,
    };
    let target_row = |value, mode, offset| DataBlockTargetIndexRow {
        id: "target".into(),
        section_ordinal: 0,
        ordinal: 0,
        frame: TargetRow::<String, u64>::new(target(value), [5, 6, 7].map(target), mode, offset)
            .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening".into(),
        opening_block_offset: 0,
    };
    let linked_rows = [
        linked("opening", 63, IndexRowMode::Form07, 100),
        linked("linked", 61, IndexRowMode::Form04, 150),
    ];
    let target_rows = [target_row(62, IndexRowMode::Form04, 125)];

    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        operation,
        |ctx| {
            super::super::data_block_column_index_tables(ctx, &linked_rows, &target_rows)
                .map(|_| ())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn column_index_order_preserves_work_refusal() {
    column_index_search_refusal("check NX column index table order");
}

#[test]
fn column_index_source_entry_search_preserves_work_refusal() {
    column_index_search_refusal("NX column index row source entries");
}

#[test]
fn column_index_source_entry_comparison_preserves_work_refusal() {
    column_index_search_refusal("NX column index source entry comparison");
}
