// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::{indexed_header, push_reference_u64, with_decode_context};

fn prefix(operation: u32, property: bool) -> Vec<u8> {
    let mut bytes = vec![0xa5; 7];
    indexed_header(&mut bytes, *b"300", 10);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(u8::from(property));
    if property {
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    bytes.extend_from_slice(&operation.to_le_bytes());
    bytes
}

fn list(bytes: &mut Vec<u8>, references: &[Option<u64>]) {
    bytes.extend_from_slice(&u32::try_from(references.len()).unwrap().to_le_bytes());
    for reference in references {
        if let Some(reference) = reference {
            push_reference_u64(bytes, *reference);
        } else {
            bytes.push(0);
        }
    }
}

fn decode(bytes: &[u8], types: &HashMap<u64, (&str, u32)>) -> Option<DesignBoundaryFill> {
    let view = View::over_retained(bytes).child(7, bytes.len()).unwrap();
    with_decode_context(|ctx| decode_prefix(ctx, view, types).unwrap())
}

#[test]
fn reads_boundary_fill_removal_lists_immediately_after_operation() {
    let types = HashMap::from([(20, (SETTINGS_TYPE_GUID, 0)), (21, (CELL_TYPE_GUID, 0))]);
    for (controls, references) in [(false, vec![]), (true, vec![Some(18), Some(19)])] {
        let mut bytes = prefix(4, true);
        list(&mut bytes, &[]);
        list(&mut bytes, &references);
        bytes.push(u8::from(controls));
        list(&mut bytes, &[Some(12)]);
        list(&mut bytes, &[Some(13), None]);
        push_reference_u64(&mut bytes, 20);
        list(&mut bytes, &[Some(21), None, Some(21)]);
        bytes.extend_from_slice(&[0xa5, 0x5a]);
        let result = decode(&bytes, &types).unwrap();
        assert_eq!(result.operation_word, 4);
        let operands = result.operands.unwrap();
        assert_eq!(operands.body_groups, [NonZeroU32::new(12)]);
        assert_eq!(operands.plane_tools, [NonZeroU32::new(13), None]);
        assert_eq!(operands.settings.get(), 20);
        assert_eq!(
            operands.cells,
            [NonZeroU32::new(21), None, NonZeroU32::new(21)]
        );
        assert_eq!(operands.suffix, [0xa5, 0x5a]);
        let removal = operands.removal.unwrap();
        assert!(removal.references[0].is_empty());
        assert_eq!(removal.references[1].len(), references.len());
        assert_eq!(removal.remove_tools, controls);
    }
}

#[test]
fn boundary_fill_keeps_operation_when_a_count_or_type_refuses_the_lists() {
    let types = HashMap::from([(20, (SETTINGS_TYPE_GUID, 0)), (21, (CELL_TYPE_GUID, 0))]);
    let wrong_types = HashMap::from([(20, (SETTINGS_TYPE_GUID, 0)), (21, (CELL_TYPE_GUID, 1))]);
    let mut bytes = prefix(2, false);
    list(&mut bytes, &[Some(12)]);
    list(&mut bytes, &[]);
    push_reference_u64(&mut bytes, 20);
    let cells_at = bytes.len();
    list(&mut bytes, &[Some(21)]);
    let operation = decode(&bytes, &types).unwrap();
    assert_eq!(operation.operation_word, 2);
    let operands = operation.operands.unwrap();
    assert_eq!(operands.cells, [NonZeroU32::new(21)]);
    assert!(operands.removal.is_none());
    let wrong = decode(&bytes, &wrong_types).unwrap();
    assert_eq!(wrong.operation_word, 2);
    assert!(wrong.operands.is_none());
    bytes[cells_at..cells_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let malformed = decode(&bytes, &types).unwrap();
    assert_eq!(malformed.operation_word, 2);
    assert!(malformed.operands.is_none());
}
