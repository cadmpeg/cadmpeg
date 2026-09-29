// SPDX-License-Identifier: Apache-2.0
use crate::om::compact::CompactIndexTarget;

use crate::om::column_row::{IndexRow, LinkedRow, TargetRow};

#[test]
fn feature_input_column_row_uses_preserve_index_row_slots() {
    use crate::native::features::feature_input_column_row_uses;
    use crate::native::features::ColumnIndexRowKind;
    use crate::native::features::FeatureInputBlock;
    use crate::native::om::column_row::DataBlockIndexRow;

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: crate::om::header_references::HeaderSlot::Two,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(7, &[7]).unwrap(),
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockIndexRow {
        id: "row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        frame: IndexRow::<String, u64>::new(
            crate::om::compact::CompactIndexAtom::from_wire(20, &[128, 20]).unwrap(),
            crate::om::discriminators::LinkedIndexFlag::Form03,
            [4, 4, 5, 6].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            100,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };

    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_input_column_row_uses(ctx, &[input], &[row], &[], &[], &[])
    })
    .unwrap();
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot.number(), 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::Index);
    assert_eq!(uses[0].column_row, "row#3");
    assert_eq!(u8::from(uses[0].row_slot), 0);
    assert_eq!(uses[0].source_offset, 108);
    assert_eq!(u8::from(uses[1].row_slot), 1);
    assert_eq!(uses[1].source_offset, 109);
}

#[test]
fn feature_input_column_row_uses_preserve_linked_row_slots() {
    use crate::native::features::feature_input_column_row_uses;
    use crate::native::features::feature_input_column_targets;
    use crate::native::features::ColumnIndexRowKind;
    use crate::native::features::FeatureInputBlock;
    use crate::native::features::FeatureInputColumnTargetRow;
    use crate::native::om::column_row::DataBlockLinkedIndexRow;
    use crate::native::om::DataBlockColumnIndexTable;

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: crate::om::header_references::HeaderSlot::Two,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(4, &[4]).unwrap(),
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockLinkedIndexRow {
        id: "linked-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        frame: LinkedRow::<String, u64>::new(
            crate::om::compact::CompactIndexAtom::from_wire(20, &[128, 20]).unwrap(),
            crate::om::discriminators::LinkedIndexDiscriminator::Form16,
            CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[4]).unwrap(),
                target: "block#4".into(),
            },
            [5, 6, 4].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            crate::om::discriminators::LinkedIndexFlag::Form03,
            crate::om::discriminators::IndexRowMode::Form04,
            100,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };

    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: row.id.clone(),
        rows: crate::native::om::column_index::ColumnIndexRows::new(
            4,
            vec!["target-row".into()],
            vec!["suffix-row".into()],
        )
        .unwrap(),
        source_entry: "entry".into(),
        source_offset: 100,
    };
    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_input_column_row_uses(
            ctx,
            std::slice::from_ref(&input),
            &[],
            std::slice::from_ref(&row),
            &[],
            &[table],
        )
    })
    .unwrap();
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot.number(), 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::LinkedIndex);
    assert_eq!(uses[0].column_row, "linked-row#3");
    assert_eq!(u8::from(uses[0].row_slot), 0);
    assert_eq!(uses[0].source_offset, 107);
    assert_eq!(u8::from(uses[1].row_slot), 3);
    assert_eq!(uses[1].source_offset, 114);
    let targets = crate::test_support::with_decode_context(|ctx| {
        feature_input_column_targets(ctx, &[input], &uses, &[row], &[])
    })
    .unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(
        targets[0].row,
        FeatureInputColumnTargetRow::Linked {
            leading_index: 20,
            leading_index_source_offset: 102,
            discriminator: crate::om::discriminators::LinkedIndexDiscriminator::Form16,
            flag: crate::om::discriminators::LinkedIndexFlag::Form03,
        }
    );
    assert_eq!(targets[0].field_indices, [5, 6, 4]);
    assert_eq!(u8::from(targets[0].mode), 4);
}

#[test]
fn feature_input_column_row_uses_preserve_target_row_slots() {
    use crate::native::features::feature_input_column_row_uses;
    use crate::native::features::feature_input_column_targets;
    use crate::native::features::ColumnIndexRowKind;
    use crate::native::features::FeatureInputBlock;
    use crate::native::features::FeatureInputColumnTargetRow;
    use crate::native::om::column_row::DataBlockTargetIndexRow;
    use crate::native::om::DataBlockColumnIndexTable;

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: crate::om::header_references::HeaderSlot::Two,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(4, &[4]).unwrap(),
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockTargetIndexRow {
        id: "target-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        frame: TargetRow::<String, u64>::new(
            CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[4]).unwrap(),
                target: "block#4".into(),
            },
            [5, 6, 4].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            crate::om::discriminators::IndexRowMode::Form07,
            100,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };

    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: "opening-row".into(),
        rows: crate::native::om::column_index::ColumnIndexRows::new(
            5,
            vec!["target-row#3".into()],
            vec!["suffix-row".into()],
        )
        .unwrap(),
        source_entry: "entry".into(),
        source_offset: 50,
    };
    let ambiguous = crate::test_support::with_decode_context(|ctx| {
        feature_input_column_row_uses(
            ctx,
            std::slice::from_ref(&input),
            &[],
            &[],
            std::slice::from_ref(&row),
            &[table.clone(), table.clone()],
        )
    })
    .unwrap();
    assert!(ambiguous.iter().all(|use_| use_.column_table.is_none()));
    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_input_column_row_uses(
            ctx,
            std::slice::from_ref(&input),
            &[],
            &[],
            std::slice::from_ref(&row),
            &[table],
        )
    })
    .unwrap();
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot.number(), 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::TargetIndex);
    assert_eq!(uses[0].column_row, "target-row#3");
    assert_eq!(uses[0].column_table.as_deref(), Some("column-table"));
    assert_eq!(u8::from(uses[0].row_slot), 0);
    assert_eq!(uses[0].source_offset, 105);
    assert_eq!(u8::from(uses[1].row_slot), 3);
    assert_eq!(uses[1].source_offset, 112);
    let targets = crate::test_support::with_decode_context(|ctx| {
        feature_input_column_targets(
            ctx,
            std::slice::from_ref(&input),
            &uses,
            &[],
            std::slice::from_ref(&row),
        )
    })
    .unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].input_block, input.id);
    assert_eq!(targets[0].column_row, "target-row#3");
    assert_eq!(targets[0].column_table, "column-table");
    assert_eq!(targets[0].field_indices, [5, 6, 4]);
    assert_eq!(
        targets[0].field_data_blocks,
        ["block#5", "block#6", "block#4"]
    );
    assert_eq!(targets[0].field_source_offsets, [110, 111, 112]);
    assert_eq!(u8::from(targets[0].mode), 7);
    assert_eq!(targets[0].row, FeatureInputColumnTargetRow::Target);
    let mut duplicate = uses.clone();
    duplicate.push(uses[0].clone());
    assert!(
        crate::test_support::with_decode_context(|ctx| feature_input_column_targets(
            ctx,
            &[input],
            &duplicate,
            &[],
            &[row]
        ))
        .unwrap()
        .is_empty()
    );
}

#[test]
fn datum_csys_column_row_uses_preserve_both_lane_offsets() {
    use crate::native::features::feature_datum_csys_column_row_uses;
    use crate::native::features::ColumnIndexRowKind;
    use crate::native::features::FeatureDatumCsysConstruction;
    use crate::native::om::column_row::DataBlockTargetIndexRow;
    use crate::native::om::DataBlockColumnIndexTable;

    let construction = FeatureDatumCsysConstruction {
        id: "construction#1".into(),
        operation_label: "operation#1".into(),
        frame: crate::om::datum_csys::DatumCsysFrame::new(
            0x16,
            181,
            std::array::from_fn(|slot| {
                (
                    crate::om::reference_index::PayloadIndexToken::from_wire(
                        u32::try_from(slot).expect("fixture value fits u32"),
                        &[0xf0, u8::try_from(slot).expect("fixture value fits u8")],
                    )
                    .unwrap(),
                    format!("block#{slot}"),
                )
            }),
        )
        .unwrap(),
    };
    let row = DataBlockTargetIndexRow {
        id: "target-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        frame: TargetRow::<String, u64>::new(
            CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[5]).unwrap(),
                target: "block#5".into(),
            },
            [6, 7, 5].map(|value| CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::read(&[value]).unwrap(),
                target: format!("block#{value}"),
            }),
            crate::om::discriminators::IndexRowMode::Form07,
            100,
        )
        .unwrap(),
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
    };
    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: "opening-row".into(),
        rows: crate::native::om::column_index::ColumnIndexRows::new(
            5,
            vec![row.id.clone()],
            vec!["suffix-row".into()],
        )
        .unwrap(),
        source_entry: "entry".into(),
        source_offset: 50,
    };

    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_datum_csys_column_row_uses(ctx, &[construction], &[], &[], &[row], &[table])
    })
    .unwrap();
    assert_eq!(uses.len(), 4);
    assert_eq!(
        uses.iter()
            .map(|use_| (u8::from(use_.construction_slot), u8::from(use_.row_slot)))
            .collect::<Vec<_>>(),
        [(5, 0), (5, 3), (6, 1), (7, 2)]
    );
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::TargetIndex);
    assert_eq!(uses[0].column_table.as_deref(), Some("column-table"));
    assert_eq!(uses[0].construction_source_offset, 205);
    assert_eq!(uses[0].row_source_offset, 105);
    assert_eq!(uses[1].construction_source_offset, 205);
    assert_eq!(uses[1].row_source_offset, 112);
}
