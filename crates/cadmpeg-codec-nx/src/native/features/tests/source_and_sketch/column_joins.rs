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

    let uses = feature_input_column_row_uses(&[input], &[row], &[], &[], &[]);
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
    let uses = {
        feature_input_column_row_uses(
            std::slice::from_ref(&input),
            &[],
            std::slice::from_ref(&row),
            &[],
            &[table],
        )
    };
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
    let targets = feature_input_column_targets(&[input], &uses, &[row], &[]);
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
    let ambiguous = {
        feature_input_column_row_uses(
            std::slice::from_ref(&input),
            &[],
            &[],
            std::slice::from_ref(&row),
            &[table.clone(), table.clone()],
        )
    };
    assert!(ambiguous.iter().all(|use_| use_.column_table.is_none()));
    let uses = {
        feature_input_column_row_uses(
            std::slice::from_ref(&input),
            &[],
            &[],
            std::slice::from_ref(&row),
            &[table],
        )
    };
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
    let targets = {
        feature_input_column_targets(
            std::slice::from_ref(&input),
            &uses,
            &[],
            std::slice::from_ref(&row),
        )
    };
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
    assert!(feature_input_column_targets(&[input], &duplicate, &[], &[row]).is_empty());
}

#[test]
fn feature_input_column_targets_join_many_inputs_through_sole_uses_and_rows() {
    use crate::native::features::feature_input_column_targets;
    use crate::native::features::ColumnIndexRowKind;
    use crate::native::features::FeatureInputBlock;
    use crate::native::features::FeatureInputColumnRowUse;
    use crate::native::om::column_row::DataBlockTargetIndexRow;
    use crate::om::column_row::ColumnRowSlot;

    let target_row = |id: String| DataBlockTargetIndexRow {
        id,
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
    let rows = (0..301)
        .map(|row| target_row(format!("target-row#{row}")))
        .chain([target_row("target-row#11".into())])
        .collect::<Vec<_>>();
    let inputs = (0..1024)
        .map(|index| FeatureInputBlock {
            id: format!("input#{index:010}"),
            operation_label: "operation#1".into(),
            input_slot: crate::om::header_references::HeaderSlot::Two,
            object: crate::om::reference_index::FeatureReferenceToken::from_wire(4, &[4]).unwrap(),
            data_block: "block#4".into(),
            source_offset: 10,
        })
        .collect::<Vec<_>>();
    let row_use = |input: &FeatureInputBlock, row: usize, slot, table: Option<&str>| {
        FeatureInputColumnRowUse {
            id: format!("use-{}-{row}", input.id),
            input_block: input.id.clone(),
            operation_label: input.operation_label.clone(),
            input_slot: input.input_slot,
            row_kind: ColumnIndexRowKind::TargetIndex,
            column_row: format!("target-row#{row}"),
            column_table: table.map(str::to_owned),
            row_slot: slot,
            data_block: input.data_block.clone(),
            source_offset: 105,
        }
    };
    let table = Some("column-table");
    // By index % 5: no use, one use, two uses, a use outside a table, and a
    // non-zero slot beside one slot-zero use.
    let mut uses = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let row = index % 300;
        match index % 5 {
            0 => {}
            1 => uses.push(row_use(input, row, ColumnRowSlot::Zero, table)),
            2 => {
                uses.push(row_use(input, row, ColumnRowSlot::Zero, table));
                uses.push(row_use(input, row + 1, ColumnRowSlot::Zero, table));
            }
            3 => uses.push(row_use(input, row, ColumnRowSlot::Zero, None)),
            _ => {
                uses.push(row_use(input, row, ColumnRowSlot::Three, table));
                uses.push(row_use(input, row, ColumnRowSlot::Zero, table));
            }
        }
    }

    let targets = feature_input_column_targets(&inputs, &uses, &[], &rows);

    let expected = inputs
        .iter()
        .enumerate()
        .filter(|(index, _)| matches!(index % 5, 1 | 4) && index % 300 != 11)
        .map(|(index, input)| (input.id.clone(), format!("target-row#{}", index % 300)))
        .collect::<Vec<_>>();
    assert_eq!(
        targets
            .iter()
            .map(|target| (target.input_block.clone(), target.column_row.clone()))
            .collect::<Vec<_>>(),
        expected
    );
    assert!(expected.len() > 400);
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

    let uses = feature_datum_csys_column_row_uses(&[construction], &[], &[], &[row], &[table]);
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

/// The pairwise rule the block-keyed join replaced: each block rescans every
/// row slot, and each matching row rescans every table.
fn pairwise_column_slots<'a>(
    data_block: &str,
    index_rows: &'a [crate::native::om::column_row::DataBlockIndexRow],
    linked_rows: &'a [crate::native::om::column_row::DataBlockLinkedIndexRow],
    target_rows: &'a [crate::native::om::column_row::DataBlockTargetIndexRow],
    tables: &'a [crate::native::om::DataBlockColumnIndexTable],
) -> Vec<(
    &'a str,
    crate::native::features::ColumnIndexRowKind,
    crate::om::column_row::ColumnRowSlot,
    u64,
    Option<&'a str>,
)> {
    use crate::native::features::ColumnIndexRowKind;
    use crate::om::column_row::ColumnRowSlot;

    let table = |row: &str| {
        let mut unique = None;
        for table in tables {
            for candidate in std::iter::once(table.opening_linked_row.as_str())
                .chain(table.rows.target_rows().iter().map(String::as_str))
                .chain(table.rows.linked_rows().iter().map(String::as_str))
            {
                if candidate == row {
                    if unique.is_some() {
                        return None;
                    }
                    unique = Some(table.id.as_str());
                }
            }
        }
        unique
    };
    let mut slots = Vec::new();
    for row in index_rows {
        for (slot, token) in ColumnRowSlot::ALL.into_iter().zip(row.frame.indices()) {
            if token.target == data_block {
                slots.push((
                    row.id.as_str(),
                    ColumnIndexRowKind::Index,
                    slot,
                    token.offset,
                    table(&row.id),
                ));
            }
        }
    }
    for row in linked_rows {
        let tokens = std::iter::once(row.frame.target_index()).chain(row.frame.indices());
        for (slot, token) in ColumnRowSlot::ALL.into_iter().zip(tokens) {
            if token.target == data_block {
                slots.push((
                    row.id.as_str(),
                    ColumnIndexRowKind::LinkedIndex,
                    slot,
                    token.offset,
                    table(&row.id),
                ));
            }
        }
    }
    for row in target_rows {
        let tokens = std::iter::once(row.frame.target_index()).chain(row.frame.indices());
        for (slot, token) in ColumnRowSlot::ALL.into_iter().zip(tokens) {
            if token.target == data_block {
                slots.push((
                    row.id.as_str(),
                    ColumnIndexRowKind::TargetIndex,
                    slot,
                    token.offset,
                    table(&row.id),
                ));
            }
        }
    }
    slots
}

#[test]
fn column_row_uses_match_the_pairwise_join_over_many_rows_and_tables() {
    use crate::native::features::feature_datum_csys_column_row_uses;
    use crate::native::features::feature_input_column_row_uses;
    use crate::native::features::format_column_relation_id;
    use crate::native::features::FeatureDatumCsysColumnRowUse;
    use crate::native::features::FeatureDatumCsysConstruction;
    use crate::native::features::FeatureInputBlock;
    use crate::native::features::FeatureInputColumnRowUse;
    use crate::native::om::column_row::{
        DataBlockIndexRow, DataBlockLinkedIndexRow, DataBlockTargetIndexRow,
    };
    use crate::native::om::DataBlockColumnIndexTable;
    use crate::om::datum_csys::DatumCsysSlot;

    let target = |value: usize| CompactIndexTarget {
        atom: crate::om::compact::CompactIndexAtom::read(&[u8::try_from(value).unwrap()]).unwrap(),
        target: format!("block#{value}"),
    };
    let block = |value: usize| value % 12 + 1;
    let index_rows = (0..60_usize)
        .map(|row| DataBlockIndexRow {
            id: format!("index-row#{}", row % 45),
            section_ordinal: 0,
            ordinal: 3,
            frame: IndexRow::<String, u64>::new(
                crate::om::compact::CompactIndexAtom::from_wire(20, &[128, 20]).unwrap(),
                crate::om::discriminators::LinkedIndexFlag::Form03,
                [row * 3, row * 5, row * 7, row].map(|value| target(block(value))),
                100 + 20 * u64::try_from(row).unwrap(),
            )
            .unwrap(),
            source_entry: "entry".into(),
            opening_data_block: "opening-block".into(),
            opening_block_offset: 8,
        })
        .collect::<Vec<_>>();
    let linked_rows = (0..50_usize)
        .map(|row| DataBlockLinkedIndexRow {
            id: format!("linked-row#{row}"),
            section_ordinal: 0,
            ordinal: 3,
            frame: LinkedRow::<String, u64>::new(
                crate::om::compact::CompactIndexAtom::from_wire(20, &[128, 20]).unwrap(),
                crate::om::discriminators::LinkedIndexDiscriminator::Form16,
                target(block(row * 2)),
                [row + 1, row * 11, row * 4].map(|value| target(block(value))),
                crate::om::discriminators::LinkedIndexFlag::Form03,
                crate::om::discriminators::IndexRowMode::Form04,
                3000 + 20 * u64::try_from(row).unwrap(),
            )
            .unwrap(),
            source_entry: "entry".into(),
            opening_data_block: "opening-block".into(),
            opening_block_offset: 8,
        })
        .collect::<Vec<_>>();
    let target_rows = (0..50_usize)
        .map(|row| DataBlockTargetIndexRow {
            id: format!("target-row#{row}"),
            section_ordinal: 0,
            ordinal: 3,
            frame: TargetRow::<String, u64>::new(
                target(block(row * 5)),
                [row * 9, row + 6, row * 2].map(|value| target(block(value))),
                crate::om::discriminators::IndexRowMode::Form07,
                6000 + 20 * u64::try_from(row).unwrap(),
            )
            .unwrap(),
            source_entry: "entry".into(),
            opening_data_block: "opening-block".into(),
            opening_block_offset: 8,
        })
        .collect::<Vec<_>>();
    // Tables overlap, and every eighth lists one target row twice.
    let tables = (0..25_usize)
        .map(|table| DataBlockColumnIndexTable {
            id: format!("column-table#{table}"),
            section_ordinal: 0,
            opening_linked_row: format!("linked-row#{table}"),
            rows: crate::native::om::column_index::ColumnIndexRows::new(
                10,
                vec![
                    format!("target-row#{}", table * 2 % 50),
                    format!(
                        "target-row#{}",
                        if table % 8 == 0 {
                            table * 2 % 50
                        } else {
                            table * 3 % 50
                        }
                    ),
                ],
                vec![
                    format!("linked-row#{}", (table + 10) % 50),
                    format!("index-row#{table}"),
                ],
            )
            .unwrap(),
            source_entry: "entry".into(),
            source_offset: 50,
        })
        .collect::<Vec<_>>();
    let inputs = (0..40_usize)
        .map(|input| FeatureInputBlock {
            id: format!("input#{input:010}"),
            operation_label: "operation#1".into(),
            input_slot: crate::om::header_references::HeaderSlot::Two,
            object: crate::om::reference_index::FeatureReferenceToken::from_wire(4, &[4]).unwrap(),
            data_block: format!("block#{}", input % 14 + 1),
            source_offset: 10,
        })
        .collect::<Vec<_>>();
    let constructions = (0..6_usize)
        .map(|construction| FeatureDatumCsysConstruction {
            id: format!("construction#{construction}"),
            operation_label: "operation#1".into(),
            frame: crate::om::datum_csys::DatumCsysFrame::new(
                0x16,
                181,
                std::array::from_fn(|slot| {
                    (
                        crate::om::reference_index::PayloadIndexToken::from_wire(
                            u32::try_from(slot).unwrap(),
                            &[0xf0, u8::try_from(slot).unwrap()],
                        )
                        .unwrap(),
                        format!("block#{}", (slot + construction * 5) % 14 + 1),
                    )
                }),
            )
            .unwrap(),
        })
        .collect::<Vec<_>>();

    let expected_inputs = inputs
        .iter()
        .flat_map(|input| {
            pairwise_column_slots(
                &input.data_block,
                &index_rows,
                &linked_rows,
                &target_rows,
                &tables,
            )
            .into_iter()
            .enumerate()
            .map(
                |(ordinal, (row, row_kind, row_slot, source_offset, table))| {
                    FeatureInputColumnRowUse {
                        id: format_column_relation_id(
                            "nx:feature-history:input-column-row-use#",
                            input.id.rsplit_once('#').map_or("unknown", |(_, key)| key),
                            None,
                            row_kind,
                            ordinal,
                        ),
                        input_block: input.id.clone(),
                        operation_label: input.operation_label.clone(),
                        input_slot: input.input_slot,
                        row_kind,
                        column_row: row.to_owned(),
                        column_table: table.map(ToOwned::to_owned),
                        row_slot,
                        data_block: input.data_block.clone(),
                        source_offset,
                    }
                },
            )
            .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(expected_inputs.len() > 500);
    assert!(expected_inputs
        .iter()
        .any(|use_| use_.column_table.is_some()));
    assert!(expected_inputs
        .iter()
        .any(|use_| use_.column_table.is_none()));
    assert_eq!(
        feature_input_column_row_uses(&inputs, &index_rows, &linked_rows, &target_rows, &tables),
        expected_inputs
    );

    let expected_constructions = constructions
        .iter()
        .flat_map(|construction| {
            DatumCsysSlot::ALL
                .into_iter()
                .zip(construction.frame.references())
                .flat_map(|(construction_slot, (_, data_block, source_offset))| {
                    pairwise_column_slots(
                        data_block,
                        &index_rows,
                        &linked_rows,
                        &target_rows,
                        &tables,
                    )
                    .into_iter()
                    .enumerate()
                    .map(
                        move |(ordinal, (row, row_kind, row_slot, row_source_offset, table))| {
                            FeatureDatumCsysColumnRowUse {
                                id: format_column_relation_id(
                                    "nx:feature-history:datum-csys-column-row-use#",
                                    construction
                                        .id
                                        .rsplit_once('#')
                                        .map_or("unknown", |(_, key)| key),
                                    Some(construction_slot),
                                    row_kind,
                                    ordinal,
                                ),
                                construction: construction.id.clone(),
                                operation_label: construction.operation_label.clone(),
                                construction_slot,
                                row_kind,
                                column_row: row.to_owned(),
                                column_table: table.map(ToOwned::to_owned),
                                row_slot,
                                data_block: data_block.clone(),
                                construction_source_offset: source_offset,
                                row_source_offset,
                            }
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(expected_constructions.len() > 200);
    assert_eq!(
        feature_datum_csys_column_row_uses(
            &constructions,
            &index_rows,
            &linked_rows,
            &target_rows,
            &tables
        ),
        expected_constructions
    );
}
