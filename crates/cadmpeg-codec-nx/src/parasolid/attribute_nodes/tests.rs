// SPDX-License-Identifier: Apache-2.0

use crate::test_support::{test_bytes::encoded_xmt, with_decode_context};

const END: [u8; 4] = [0, 1, 0, 1];

fn header() -> Vec<u8> {
    let mut bytes = b"PS".to_vec();
    for field in [": TRANSMIT FILE (partition) attributes", "SCH_999_13006"] {
        bytes.extend(u32::try_from(field.len()).unwrap().to_be_bytes());
        bytes.extend(field.as_bytes());
    }
    bytes.extend(0u32.to_be_bytes());
    bytes
}

/// A variable-length node: type, element count, index, then `payload`.
fn counted(kind: u16, count: u32, index: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = kind.to_be_bytes().to_vec();
    bytes.extend(count.to_be_bytes());
    bytes.extend(encoded_xmt(index));
    bytes.extend_from_slice(payload);
    bytes
}

/// An `ATTRIBUTE` owned by node 300, whose index cell starts with `01`, with
/// an `INT_VALUES` and a `CHAR_VALUES` field.
fn attributed_stream() -> (Vec<u8>, [usize; 3]) {
    let mut fixed = 7u32.to_be_bytes().to_vec();
    for pointer in [50, 300, 1, 1, 1, 1, 101, 102] {
        fixed.extend(encoded_xmt(pointer));
    }
    let mut bytes = header();
    let attribute = bytes.len();
    bytes.extend(counted(81, 2, 40, &fixed));
    let integers = bytes.len();
    bytes.extend(counted(82, 1, 101, &681u32.to_be_bytes()));
    let string = bytes.len();
    bytes.extend(counted(84, 3, 102, b"abc"));
    bytes.extend(END);
    (bytes, [attribute, integers, string])
}

#[test]
fn a_terminated_attribute_with_a_high_owner_index_is_read() {
    with_decode_context(|_ctx| {
        let (bytes, [attribute, integers, _]) = attributed_stream();
        let records = crate::parasolid::entity_51_records(&bytes);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(
            (record.offset, record.byte_len, u32::from(record.xmt)),
            (attribute, integers - attribute, 40)
        );
        assert_eq!(record.definition_xmt, 50);
        assert_eq!(record.leading_references, [300, 1, 1, 1, 1]);
        assert_eq!(record.trailing_references.values(), [101, 102]);
    });
}

#[test]
fn terminated_values_span_their_nodes() {
    with_decode_context(|_ctx| {
        let (bytes, [_, integers, string]) = attributed_stream();
        let values = crate::parasolid::referenced_value_records(&bytes).unwrap();
        assert_eq!(values.integers.len(), 1);
        assert_eq!(
            (values.integers[0].offset, values.integers[0].byte_len),
            (integers, string - integers)
        );
        assert_eq!(values.integers[0].value.as_slice(), [681]);
        assert_eq!(values.strings.len(), 1);
        let record = &values.strings[0];
        assert_eq!(
            (record.offset, record.byte_len, record.value.as_str()),
            (string, bytes.len() - END.len() - string, "abc")
        );
    });
}

#[test]
fn a_terminated_definition_joins_its_key() {
    with_decode_context(|_ctx| {
        let mut fixed = encoded_xmt(1);
        fixed.extend(encoded_xmt(60));
        fixed.extend(9u32.to_be_bytes());
        fixed.extend([0; 8]);
        fixed.extend(encoded_xmt(1));
        fixed.extend([1; 14]);
        fixed.extend([1, 2]);
        let mut bytes = header();
        let key = bytes.len();
        bytes.extend(counted(79, 4, 60, b"NAME"));
        let definition = bytes.len();
        bytes.extend(counted(80, 2, 50, &fixed));
        bytes.extend(END);
        let definitions = crate::parasolid::attribute_definitions(&bytes);
        assert_eq!(definitions.len(), 1);
        let record = &definitions[0];
        assert_eq!(
            (
                record.offset,
                u32::from(record.xmt),
                record.identifier_offset
            ),
            (definition, 50, key)
        );
        assert_eq!((record.name.as_str(), record.type_id.get()), ("NAME", 9));
        assert_eq!(record.next_definition_xmt, None);
        assert_eq!(record.field_names_xmt, None);
        assert_eq!(
            record.legal_owner_flags,
            crate::parasolid::LegalOwnerFlags::Fourteen([true; 14])
        );
        assert_eq!(
            record.field_codes,
            [1, 2].map(
                |code| crate::parasolid::attribute_field::AttributeField::try_from(code).unwrap()
            )
        );
    });
}

#[test]
fn a_terminated_field_name_list_keeps_its_names() {
    with_decode_context(|_ctx| {
        let mut names = encoded_xmt(102);
        names.extend(encoded_xmt(103));
        let mut bytes = header();
        let list = bytes.len();
        bytes.extend(counted(99, 2, 70, &names));
        let end = bytes.len();
        bytes.extend(END);
        let records = crate::parasolid::field_names_records(&bytes);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(
            (record.offset, record.byte_len, u32::from(record.xmt)),
            (list, end - list, 70)
        );
        assert_eq!(
            record
                .name_xmts
                .as_slice()
                .iter()
                .map(|name| u32::from(*name))
                .collect::<Vec<_>>(),
            [102, 103]
        );
    });
}
