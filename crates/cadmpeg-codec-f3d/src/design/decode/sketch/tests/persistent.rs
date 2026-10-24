// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::sketch::{
    decode_persistent_references_from_stream, finish_persistent_references, SourcePointers,
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

fn scan_references(
    bytes: &[u8],
) -> Result<
    (
        Vec<PersistentReference>,
        SourcePointers,
        Vec<cadmpeg_core::decode::RecordCapture>,
    ),
    cadmpeg_core::CodecError,
> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    let mut indexed = Vec::new();
    let mut pointers = SourcePointers::new();
    decode_persistent_references_from_stream(
        &ctx,
        0,
        "BulkStream.dat",
        root,
        &mut indexed,
        &mut pointers,
    )?;
    let captures = ctx.take_record_captures()?;
    Ok((finish_persistent_references(indexed), pointers, captures))
}

#[test]
fn persistent_reference_scan_preserves_byte_order_after_kind_scan() {
    let (bytes, _) = reference_bytes();
    let (references, ..) = scan_references(&bytes).expect("two typed references");
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].value, 71);
    assert_eq!(references[1].value, 29);
    assert_eq!(
        references[0].id,
        crate::ids::native_persistent_reference_id("BulkStream.dat", 0)
    );
}

/// #225 comment 6358238990 item 4 and #214: each reference is parsed inside
/// its record scope; its offsets are `record_layout` pointers and not
/// metadata; the property grammar (name, the u32 type-name length 23, the
/// type name and the u64 value) consumes every byte of its extent.
#[test]
fn a_persistent_reference_capture_holds_its_source_offsets_and_reads_its_property() {
    let (bytes, second) = reference_bytes();
    let (references, pointers, captures) = scan_references(&bytes).expect("two typed references");
    let property = |name: &str| name.len() + 4 + "IntrinsicMetaTypeuint64".len() + 8;
    let extents = [
        0..property("crv_primary_id"),
        second..second + property("pt_tag"),
    ];
    assert_eq!(extents[0].end, second);
    assert_eq!(extents[1].end, bytes.len());
    assert_eq!(captures.len(), 2);
    for (reference, extent) in references.iter().zip(&extents) {
        let value_offset = extent.len() - 8;
        assert_eq!(
            pointers[&reference.id],
            std::collections::BTreeMap::from([
                ("byte_offset".to_owned(), u64_from_index(extent.start)),
                ("value_offset".to_owned(), u64_from_index(value_offset)),
            ])
        );
        let capture = captures
            .iter()
            .find(|capture| capture.id() == reference.id)
            .expect("reference capture");
        assert_eq!(capture.offset(), u64_from_index(extent.start));
        assert_eq!(capture.byte_len(), u64_from_index(extent.len()));
        assert_eq!(
            capture.reads(),
            &std::collections::BTreeMap::from([(
                "file".to_owned(),
                vec![u64_from_index(extent.start)..u64_from_index(extent.end)]
            )])
        );
        let wire = serde_json::to_value(reference).unwrap();
        assert!(wire.get("byte_offset").is_none());
        assert!(wire.get("value_offset").is_none());
    }
}

/// A name that a property grammar does not follow leaves no reference and no
/// capture.
#[test]
fn a_persistent_reference_name_without_its_property_leaves_no_capture() {
    let mut bytes = b"pt_tag".to_vec();
    bytes.extend_from_slice(&23u32.to_le_bytes());
    bytes.extend_from_slice(b"IntrinsicMetaTypeuint32");
    bytes.extend_from_slice(&7u64.to_le_bytes());
    let (references, pointers, captures) = scan_references(&bytes).expect("no reference");
    assert!(references.is_empty());
    assert!(pointers.is_empty());
    assert!(captures.is_empty());
}
