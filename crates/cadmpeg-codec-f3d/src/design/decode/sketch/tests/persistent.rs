// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{
    decode_persistent_references_from_stream, finish_persistent_references,
};
use crate::records::references::PersistentReference;

/// Two compact persistent-reference properties and the offset where the
/// second starts.
fn reference_bytes() -> (Vec<u8>, usize) {
    let mut bytes = Vec::new();
    let mut second = 0;
    for (name, value) in [("crv_primary_id", 71u64), ("pt_tag", 29)] {
        second = bytes.len();
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    (bytes, second)
}

fn scan_references(bytes: &[u8]) -> Result<Vec<PersistentReference>, cadmpeg_core::CodecError> {
    let mut indexed = Vec::new();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    decode_persistent_references_from_stream(
        &ctx,
        &mut parts,
        0,
        "BulkStream.dat",
        cadmpeg_core::decode::View::over_retained(bytes),
        &[crate::metastream::PrimaryRecordFrame {
            entity_id: 1,
            start: 0,
            member_end: bytes.len(),
            end: bytes.len(),
        }],
        &mut indexed,
    )?;
    Ok(finish_persistent_references(indexed))
}

#[test]
fn persistent_reference_scan_preserves_byte_order_after_kind_scan() {
    let (bytes, _) = reference_bytes();
    let references = scan_references(&bytes).expect("two typed references");
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].value, 71);
    assert_eq!(references[1].value, 29);
    assert_eq!(
        references[0].id,
        crate::ids::native_persistent_reference_id("BulkStream.dat", 0)
    );
}

/// A name that a property grammar does not follow leaves no reference.
#[test]
fn a_persistent_reference_name_without_its_property_leaves_no_capture() {
    let mut bytes = b"pt_tag".to_vec();
    bytes.extend_from_slice(&23u32.to_le_bytes());
    bytes.extend_from_slice(b"IntrinsicMetaTypeuint32");
    bytes.extend_from_slice(&7u64.to_le_bytes());
    let references = scan_references(&bytes).expect("no reference");
    assert!(references.is_empty());
}

#[test]
fn truncated_reference_keeps_a_sibling_record() {
    let mut first = b"pt_tag".to_vec();
    first.extend_from_slice(&23u32.to_le_bytes());
    first.extend_from_slice(b"IntrinsicMetaTypeuint64");
    first.extend_from_slice(&[1; 4]);
    let second_start = first.len();
    let (second, _) = reference_bytes();
    first.extend_from_slice(&second);
    let frames = [
        crate::metastream::PrimaryRecordFrame {
            entity_id: 1,
            start: 0,
            member_end: second_start,
            end: second_start,
        },
        crate::metastream::PrimaryRecordFrame {
            entity_id: 2,
            start: second_start,
            member_end: first.len(),
            end: first.len(),
        },
    ];
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let mut captured = Vec::new();
    decode_persistent_references_from_stream(
        &ctx,
        &mut parts,
        0,
        "BulkStream.dat",
        cadmpeg_core::decode::View::over_retained(&first),
        &frames,
        &mut captured,
    )
    .unwrap();
    let references = finish_persistent_references(captured);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].value, 71);
    assert_eq!(references[1].value, 29);
    let (_, native, fidelity, losses) = parts.into_parts();
    assert_eq!(losses.len(), 1);
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(
        native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
        1
    );
}

#[test]
fn a_property_cannot_cross_primary_record_boundaries() {
    let mut bytes = b"pt_tag".to_vec();
    bytes.extend_from_slice(&23_u32.to_le_bytes());
    bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
    bytes.extend_from_slice(&7_u64.to_le_bytes());
    let frames = [
        crate::metastream::PrimaryRecordFrame {
            entity_id: 1,
            start: 0,
            member_end: 3,
            end: 3,
        },
        crate::metastream::PrimaryRecordFrame {
            entity_id: 2,
            start: 3,
            member_end: bytes.len(),
            end: bytes.len(),
        },
    ];
    let mut references = Vec::new();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    decode_persistent_references_from_stream(
        &ctx,
        &mut parts,
        0,
        "BulkStream.dat",
        cadmpeg_core::decode::View::over_retained(&bytes),
        &frames,
        &mut references,
    )
    .unwrap();
    assert!(references.is_empty());
}
