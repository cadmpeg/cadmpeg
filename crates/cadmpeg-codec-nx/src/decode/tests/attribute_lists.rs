// SPDX-License-Identifier: Apache-2.0
//! Topology attribute lists that do not decode are reported.

use crate::loss::NxLossCode;
use crate::test_support::test_bytes::encoded_xmt;
use crate::test_support::test_prt::prt_with_streams;
use crate::NxCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

const END: [u8; 4] = [0, 1, 0, 1];
const VERTEX: u32 = 300;
const LIST: u32 = 40;
const POINT: u32 = 301;

fn header(schema: &str, embedded: bool) -> Vec<u8> {
    let mut bytes = b"PS".to_vec();
    for field in [": TRANSMIT FILE (partition) attributes", schema] {
        bytes.extend(
            u32::try_from(field.len())
                .expect("field length")
                .to_be_bytes(),
        );
        bytes.extend(field.as_bytes());
    }
    if embedded {
        bytes.extend(250u16.to_be_bytes());
    }
    bytes.extend(0u32.to_be_bytes());
    bytes
}

/// A `VERTEX` whose attribute list is `LIST`, then its `POINT`; `program` is
/// the schema byte of each.
fn vertex(program: &[u8]) -> Vec<u8> {
    let mut bytes = 18u16.to_be_bytes().to_vec();
    bytes.extend_from_slice(program);
    bytes.extend(encoded_xmt(VERTEX));
    bytes.extend(1u32.to_be_bytes());
    for pointer in [LIST, 1, 1, 1, POINT] {
        bytes.extend(encoded_xmt(pointer));
    }
    bytes.extend(1.0e-6f64.to_be_bytes());
    bytes.extend(encoded_xmt(1));
    bytes.extend(29u16.to_be_bytes());
    bytes.extend_from_slice(program);
    bytes.extend(encoded_xmt(POINT));
    bytes.extend(2u32.to_be_bytes());
    for pointer in [1, VERTEX, 1, 1] {
        bytes.extend(encoded_xmt(pointer));
    }
    for coordinate in [0.0f64, 0.0, 0.0] {
        bytes.extend(coordinate.to_be_bytes());
    }
    bytes
}

/// A non-pointer field descriptor of `n_elements` elements of `data_type`.
fn descriptor(name: &str, n_elements: u16, data_type: u8) -> Vec<u8> {
    let mut bytes = vec![u8::try_from(name.len()).expect("name length")];
    bytes.extend(name.as_bytes());
    bytes.extend(0u16.to_be_bytes());
    bytes.extend((n_elements + 1).to_be_bytes());
    bytes.extend([1, data_type]);
    if n_elements == 1 {
        bytes.push(1);
    }
    bytes
}

/// A first-use variable node whose schema declaration is `program`.
fn declared(kind: u16, program: &[u8], count: u32, index: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = kind.to_be_bytes().to_vec();
    bytes.extend_from_slice(program);
    bytes.extend(count.to_be_bytes());
    bytes.extend(encoded_xmt(index));
    bytes.extend_from_slice(payload);
    bytes
}

/// The fixed fields of `ATTRIBUTE` `LIST`, owned by `VERTEX`, with no
/// trailing fields.
fn attribute_fields() -> Vec<u8> {
    let mut bytes = 7u32.to_be_bytes().to_vec();
    for pointer in [50, VERTEX, 1, 1, 1, 1, 1] {
        bytes.extend(encoded_xmt(pointer));
    }
    bytes
}

/// The fixed fields of `ATTRIB_DEF` 50 that names `KEY` 60.
fn definition_fields() -> Vec<u8> {
    let mut fixed = encoded_xmt(1);
    fixed.extend(encoded_xmt(60));
    fixed.extend(9u32.to_be_bytes());
    fixed.extend([0; 8]);
    fixed.extend(encoded_xmt(1));
    fixed.extend([1; 14]);
    fixed.extend([1, 2]);
    fixed
}

/// A terminated stream: `VERTEX`, `KEY`, `ATTRIB_DEF` with `definition`
/// schema, and `ATTRIBUTE` with `attribute` schema.
fn terminated_stream(attribute: &[u8], definition: &[u8]) -> Vec<u8> {
    let count = if attribute == [255] { 1 } else { 2 };
    let mut bytes = header("SCH_999_999_13006", true);
    bytes.extend(declared(79, &[255], 4, 60, b"NAME"));
    bytes.extend(declared(80, definition, 2, 50, &definition_fields()));
    bytes.extend(declared(81, attribute, count, LIST, &attribute_fields()));
    bytes.extend(vertex(&[255]));
    bytes.extend(END);
    bytes
}

/// A terminated stream with one `POINT`, so that the part transfers geometry.
fn carrier_stream() -> Vec<u8> {
    let mut bytes = header("SCH_999_13006", false);
    bytes.extend(29u16.to_be_bytes());
    bytes.extend(encoded_xmt(2));
    bytes.extend(1u32.to_be_bytes());
    for pointer in [1, 1, 1, 1] {
        bytes.extend(encoded_xmt(pointer));
    }
    for coordinate in [0.0f64, 0.0, 0.0] {
        bytes.extend(coordinate.to_be_bytes());
    }
    bytes.extend(END);
    bytes
}

fn reports_unresolved_list(stream: &[u8]) -> bool {
    let mut cursor = Cursor::new(prt_with_streams(&[&carrier_stream(), stream]));
    let result = NxCodec
        .decode(&mut cursor, &DecodeOptions::default())
        .expect("decode the part");
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == NxLossCode::AttributeListUnresolved.kind())
}

/// An `ATTRIBUTE` schema edit that declares `fields` as a character field.
fn character_fields_program() -> Vec<u8> {
    let mut program = vec![8];
    program.extend([b'C'; 7]);
    program.extend(*b"DA");
    program.extend(descriptor("fields", 1, b'c'));
    program.push(b'Z');
    program
}

/// An `ATTRIB_DEF` schema edit that declares `legal_owners` as unsigned bytes.
fn unsigned_owners_program() -> Vec<u8> {
    let mut program = vec![7, b'C', b'C', b'C', b'C', b'C', b'D', b'I'];
    program.extend(descriptor("legal_owners", 14, b'u'));
    program.extend(*b"CZ");
    program
}

/// An unterminated stream whose type-81 list starts with an owner index cell
/// that starts with `01` and does not frame in the status-prefixed form.
fn unsettled_stream() -> Vec<u8> {
    let mut bytes = header("SCH_TEST_1_9999", false);
    bytes.extend(vertex(&[]));
    bytes.extend([0x00, 0x51]);
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(encoded_xmt(LIST));
    bytes.extend(7u32.to_be_bytes());
    for pointer in [50, VERTEX, 1, 1, 1, 1, 101] {
        bytes.extend(encoded_xmt(pointer));
    }
    bytes
}

#[test]
fn a_resolved_topology_attribute_list_is_not_reported() {
    assert!(!reports_unresolved_list(&terminated_stream(&[255], &[255])));
}

#[test]
fn a_referenced_attribute_with_an_incompatible_schema_is_reported() {
    assert!(reports_unresolved_list(&terminated_stream(
        &character_fields_program(),
        &[255]
    )));
}

#[test]
fn a_referenced_definition_with_an_incompatible_schema_is_reported() {
    assert!(reports_unresolved_list(&terminated_stream(
        &[255],
        &unsigned_owners_program()
    )));
}

#[test]
fn an_unterminated_list_with_an_unsettled_reference_form_is_reported() {
    assert_eq!(encoded_xmt(VERTEX)[0], 1);
    assert!(reports_unresolved_list(&unsettled_stream()));
}
