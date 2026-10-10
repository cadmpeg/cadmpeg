// SPDX-License-Identifier: Apache-2.0
//! Optional naming metadata must not gate independently framed shape payloads.

use super::*;
use crate::test_support::test_archive::rewrite_entry;

#[test]
fn malformed_naming_metadata_keeps_geometry_and_valid_native_state() {
    let original = rewrite_entry(GEOMETRY, "Document.xml", |data| {
        std::str::from_utf8(data)
            .unwrap()
            .replacen(
                "<Part ElementMap=",
                "<Part HasherIndex=\"0\" ElementMap=",
                1,
            )
            .into_bytes()
    });
    let expected = FcstdCodec
        .decode(&mut Cursor::new(&original), &DecodeOptions::default())
        .unwrap();
    assert!(!expected.ir().model.faces.is_empty());
    for (before, after) in [
        ("threshold=\"0\"", "threshold=\"invalid\""),
        ("saveall=\"0\"", "saveall=\"invalid\""),
        ("HasherIndex=\"0\"", "HasherIndex=\"invalid\""),
        (
            "HasherIndex=\"0\"",
            "HasherIndex=\"9999999999999999999999999\"",
        ),
        ("HasherIndex=\"0\"", "HasherIndex=\"9999\""),
    ] {
        let source = rewrite_entry(&original, "Document.xml", |data| {
            let text = std::str::from_utf8(data).unwrap();
            assert!(text.contains(before), "missing {before}");
            text.replace(before, after).into_bytes()
        });
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(recovered.ir().model, expected.ir().model, "{after}");
        assert!(recovered
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "element-map.metadata-unresolved"));
        assert_valid_document(recovered.ir());
        assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
        let wire = serde_json::to_value(recovered.ir()).unwrap();
        let roundtrip = serde_json::from_value::<cadmpeg_ir::CadIr>(wire).unwrap();
        assert!(crate::test_support::validate_native(&roundtrip).is_empty());
    }
}

#[test]
fn naming_metadata_recovery_preserves_resource_refusals() {
    crate::test_support::assert_retained_refusal_at(
        &[],
        "FreeCAD naming metadata diagnostic",
        |ctx| {
            super::super::recover_metadata::<()>(
                ctx,
                Err(CodecError::Malformed("unreadable".into())),
                &mut Vec::new(),
                "StringHasher payload",
            )
        },
    );
    crate::test_support::assert_collection_refusal_at(
        &[],
        "FreeCAD naming metadata losses",
        |ctx| {
            super::super::recover_metadata::<()>(
                ctx,
                Err(CodecError::Malformed("unreadable".into())),
                &mut Vec::new(),
                "StringHasher payload",
            )
        },
    );
    let source = rewrite_entry(GEOMETRY, "Document.xml", |data| {
        std::str::from_utf8(data)
            .unwrap()
            .replacen(
                "threshold=\"0\" count=\"0\"",
                "threshold=\"0\" count=\"10000001\"",
                1,
            )
            .into_bytes()
    });
    assert!(matches!(
        FcstdCodec.decode(&mut Cursor::new(source), &DecodeOptions::default()),
        Err(cadmpeg_ir::DecodeFailure::Codec(CodecError::ResourceLimit(
            _
        )))
    ));
}

#[test]
fn unreadable_naming_carriers_keep_geometry_and_contiguous_table_indices() {
    let expected = FcstdCodec
        .decode(&mut Cursor::new(GEOMETRY), &DecodeOptions::default())
        .unwrap();
    for after in [
        "<StringHasher saveall=\"0\" threshold=\"0\" count=\"1\">malformed</StringHasher>",
        "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\" file=\"missing-table.txt\"/>",
        "<StringHasher new=\"1\" saveall=\"0\" threshold=\"0\"/>",
    ] {
        let source = rewrite_entry(GEOMETRY, "Document.xml", |data| {
            let text = std::str::from_utf8(data).unwrap();
            assert!(text.contains(
                "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\"></StringHasher>"
            ));
            text.replace(
                "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\"></StringHasher>",
                after,
            )
            .into_bytes()
        });
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(recovered.ir().model, expected.ir().model);
        let tables = recovered
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::StringTableRecord>("string_tables")
            .unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].index, 0);
        assert_eq!(tables[0].entries(), None);
        assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
    }
}

#[test]
fn damaged_table_framing_does_not_erase_a_readable_peer() {
    let source = rewrite_entry(GEOMETRY, "Document.xml", |data| {
        let text = std::str::from_utf8(data).unwrap();
        let xml = roxmltree::Document::parse(text).unwrap();
        let part = xml
            .descendants()
            .find(|node| node.has_tag_name("Part"))
            .unwrap();
        let mut text = text.to_owned();
        text.insert_str(
            part.range().end,
            "<StringHasher saveall=\"1\" threshold=\"7\" count=\"1\">1.c stable</StringHasher>",
        );
        text.replace(
            "<StringHasher saveall=\"0\" threshold=\"0\" count=\"0\"></StringHasher>",
            "<StringHasher new=\"1\" saveall=\"0\" threshold=\"0\"/>",
        )
        .into_bytes()
    });
    let recovered = FcstdCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    let tables = recovered
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    assert_eq!(tables.len(), 2);
    assert_eq!(tables[0].entries(), None);
    assert_eq!(tables[1].index, 1);
    assert_eq!(tables[1].entries().unwrap()[0].payload, "stable");
    assert!(tables[1].owner_property.is_some());
    assert_eq!(recovered.ir().model.faces.len(), 48);
    assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
}
