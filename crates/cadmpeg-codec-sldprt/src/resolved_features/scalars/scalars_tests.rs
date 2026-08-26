//! Tests for the `scalars` module.

use super::super::names::object_names;
use super::super::{COMPACT_SCALAR_HEADER, NAME_MARKER, SCALAR_HEADER, VALUE_ONLY_SCALAR_HEADER};
use super::named_scalars;
use crate::records::FeatureInputOperandKind;

#[test]
fn scalar_trailer_is_relative_to_variable_length_name() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(3);
    for unit in "D10".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&0.025f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 59, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&42u32.to_le_bytes());
    payload[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
    for (relative, index) in [(35usize, 7u16), (47, 9)] {
        payload[trailer + relative..trailer + relative + 2].copy_from_slice(&[0xd6, 0x80]);
        payload[trailer + relative + 2..trailer + relative + 4]
            .copy_from_slice(&index.to_le_bytes());
        payload[trailer + relative + 4..trailer + relative + 8].fill(0xff);
    }
    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.object_id, 42);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert_eq!(scalar.entity_indices, [7, 9]);
}

#[test]
fn compact_scalar_header_ends_at_the_value() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(COMPACT_SCALAR_HEADER);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 51, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&115u32.to_le_bytes());
    payload[trailer + 21..trailer + 27].copy_from_slice(&[1, 0, 0, 0, 2, 0]);
    payload[trailer + 27] = 0;
    payload[trailer + 35..trailer + 37].copy_from_slice(&0x8152u16.to_le_bytes());
    payload[trailer + 37..trailer + 39].copy_from_slice(&7u16.to_le_bytes());
    payload[trailer + 39..trailer + 43].fill(0xff);
    payload[trailer + 43..trailer + 45].copy_from_slice(&0x8152u16.to_le_bytes());
    payload[trailer + 45..trailer + 47].copy_from_slice(&9u16.to_le_bytes());
    payload[trailer + 47..trailer + 51].fill(0xff);

    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.value, 0.0);
    assert_eq!(scalar.object_id, 115);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert!(scalar.entity_indices.is_empty());
    assert_eq!(
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .collect::<Vec<_>>(),
        [
            (FeatureInputOperandKind::Native(0x8152), 7),
            (FeatureInputOperandKind::Native(0x8152), 9),
        ]
    );
}

#[test]
fn value_only_scalar_header_ends_at_the_value() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(VALUE_ONLY_SCALAR_HEADER);
    payload.extend_from_slice(&0.0f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 24, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&132u32.to_le_bytes());

    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.value, 0.0);
    assert_eq!(scalar.object_id, 132);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Native);
    assert!(scalar.operands.is_empty());
}

#[test]
fn legacy_scalar_layout_carries_shifted_role_and_operand() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(SCALAR_HEADER);
    payload.extend_from_slice(&0.004f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 48, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&28u32.to_le_bytes());
    payload[trailer + 24..trailer + 30].copy_from_slice(&[0x0f, 0, 0, 0, 2, 0]);
    payload[trailer + 30] = 0;
    payload[trailer + 36..trailer + 38].copy_from_slice(&[0xcc, 0x80]);
    payload[trailer + 38..trailer + 40].copy_from_slice(&0u16.to_le_bytes());
    payload[trailer + 40..trailer + 44].fill(0xff);

    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert_eq!(scalar.operands.len(), 1);
    assert_eq!(scalar.operands[0].offset, (trailer + 36) as u64);
    assert_eq!(
        scalar.operands[0].kind,
        crate::records::FeatureInputOperandKind::Native(0x80cc)
    );
    assert_eq!(scalar.operands[0].entity_index, 0);
}

#[test]
fn shifted_value_only_scalar_carries_standard_operand_cells() {
    let mut payload = Vec::new();
    payload.extend_from_slice(NAME_MARKER);
    payload.push(2);
    for unit in "D1".encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(VALUE_ONLY_SCALAR_HEADER);
    payload.extend_from_slice(&[0; 4]);
    let value_offset = payload.len();
    payload.extend_from_slice(&0.045f64.to_le_bytes());
    let trailer = payload.len();
    payload.resize(trailer + 35 + 2 * 12, 0);
    payload[trailer + 3..trailer + 7].copy_from_slice(&70u32.to_le_bytes());
    payload[trailer + 21..trailer + 27].copy_from_slice(&[1, 0, 0, 0, 2, 0]);
    payload[trailer + 27] = 0;
    for (relative, index) in [(35usize, 0u16), (47, 1)] {
        payload[trailer + relative..trailer + relative + 2]
            .copy_from_slice(&0x81b2u16.to_le_bytes());
        payload[trailer + relative + 2..trailer + relative + 4]
            .copy_from_slice(&index.to_le_bytes());
        payload[trailer + relative + 4..trailer + relative + 8].fill(0xff);
    }

    let names = object_names(&payload, "lane");
    let scalars = named_scalars(&payload, "lane", &names);
    let [scalar] = scalars.as_slice() else {
        panic!("expected one scalar");
    };
    assert_eq!(scalar.offset, value_offset as u64);
    assert_eq!(scalar.object_id, 70);
    assert_eq!(scalar.role, crate::records::FeatureInputScalarRole::Driving);
    assert!(scalar.entity_indices.is_empty());
    assert_eq!(
        scalar
            .operands
            .iter()
            .map(|operand| (operand.kind, operand.entity_index))
            .collect::<Vec<_>>(),
        [
            (FeatureInputOperandKind::Native(0x81b2), 0),
            (FeatureInputOperandKind::Native(0x81b2), 1),
        ]
    );
}
