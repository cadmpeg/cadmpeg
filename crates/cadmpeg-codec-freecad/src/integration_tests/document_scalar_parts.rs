// SPDX-License-Identifier: Apache-2.0
//! Document Bool and Float values are produced within actual source-owned parts.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

fn document(properties: &str, count: usize) -> String {
    format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='{count}'>{properties}</Properties><Objects Count='0'/><ObjectData Count='0'/></Document>")
}

fn archive(xml: &str) -> Vec<u8> {
    zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())])
}

#[test]
fn bool_and_float_native_and_neutral_values_flow_from_exact_original_attribute_reads() {
    for (type_name, tag, raw, held, expected) in [
        (
            "App::PropertyBool",
            "Bool",
            "tr&#117;e",
            "true",
            AttributeValue::Boolean(true),
        ),
        (
            "App::PropertyBool",
            "Bool",
            "TrUe",
            "TrUe",
            AttributeValue::Boolean(true),
        ),
        (
            "App::PropertyBool",
            "Bool",
            "0",
            "0",
            AttributeValue::Boolean(false),
        ),
        (
            "App::PropertyFloat",
            "Float",
            "1&#x2e;25",
            "1.25",
            AttributeValue::float(1.25).unwrap(),
        ),
        (
            "App::PropertyFloat",
            "Float",
            "-2.5",
            "-2.5",
            AttributeValue::float(-2.5).unwrap(),
        ),
    ] {
        let property = format!("<Property name='Input' type='{type_name}' Vendor='unheld'><{tag} value='{raw}' extra='native-only'/></Property>");
        let xml = document(&property, 1);
        let bytes = archive(&xml);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let (scan, output) = crate::container::scan_for_decode(&ctx, root).unwrap();
        let properties = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let [kept] = properties.as_slice() else {
            panic!("one kept scalar")
        };
        let pure = crate::persistence::parse_document(
            &scan.document_xml,
            crate::persistence::Vocabulary::Objects,
        )
        .unwrap();
        assert_eq!(properties, pure.properties);
        assert_eq!(kept.type_name, type_name);
        assert_eq!(kept.values()[0].attributes["value"], held);
        assert_eq!(kept.values()[0].attributes["extra"], "native-only");
        let [attribute] = output.model().attributes.as_slice() else {
            panic!("one live scalar attribute")
        };
        assert_eq!(attribute.values, [Some(expected)]);
        assert_eq!(attribute.native_ref.as_deref(), Some(kept.id.as_str()));
    }
}

#[test]
fn changing_unheld_property_header_bytes_keeps_scalar_content_equal_without_read_credit() {
    let xml = document("<Property name='Boolean' type='App::PropertyBool' Vendor='first'><Bool value='true'/></Property><Property name='Float' type='App::PropertyFloat' Vendor='first'><Float value='1.25'/></Property>", 2);
    let baseline = crate::FcstdCodec
        .decode(&mut Cursor::new(archive(&xml)), &DecodeOptions::default())
        .unwrap();
    let changed = xml.replace("Vendor='first'", "Vendor='other'");
    assert_ne!(changed, xml);
    let decoded = crate::FcstdCodec
        .decode(
            &mut Cursor::new(archive(&changed)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert_eq!(
        properties,
        baseline
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap()
    );
    assert_eq!(
        decoded.ir().model.attributes,
        baseline.ir().model.attributes
    );
}

#[test]
fn invalid_scalar_give_up_keeps_exact_float_bool_integer_and_string_peers() {
    let peers = "<Property name='Boolean' type='App::PropertyBool'><Bool value='true'/></Property><Property name='Float' type='App::PropertyFloat'><Float value='1.25'/></Property><Property name='Integer' type='App::PropertyInteger'><Integer value='17'/></Property><Property name='String' type='App::PropertyString'><String value='peer'/></Property>";
    let baseline = crate::FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document(peers, 4))),
            &DecodeOptions::default(),
        )
        .unwrap();
    for bad in [
        "<Property name='Bad' type='App::PropertyBool'><Bool value='unknown'/></Property>",
        "<Property name='Bad' type='App::PropertyBool'><Bool/></Property>",
        "<Property name='Bad' type='App::PropertyFloat'><Float value='NaN'/></Property>",
        "<Property name='Bad' type='App::PropertyFloat'><Float value='1e999'/></Property>",
        "<Property name='Bad' type='App::PropertyFloat'><Float/></Property>",
    ] {
        let bad_dom = roxmltree::Document::parse(bad).unwrap();
        let value_range = bad_dom
            .root_element()
            .first_element_child()
            .unwrap()
            .range();
        let raw_value = &bad[value_range.clone()];
        let xml = document(&format!("{peers}{bad}"), 5);
        let bytes = archive(&xml);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
        let properties = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        assert_eq!(properties.len(), 5);
        let bad_property = properties
            .iter()
            .find(|property| property.name == "Bad")
            .unwrap();
        assert!(bad_property.body.is_none());
        assert_eq!(
            bad_property.type_name,
            bad_dom.root_element().attribute("type").unwrap()
        );
        let readable_peers = properties
            .iter()
            .filter(|property| property.name != "Bad")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            readable_peers,
            baseline
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::PropertyRecord>("properties")
                .unwrap()
        );
        assert_eq!(
            decoded.ir().model.attributes,
            baseline.ir().model.attributes
        );
        let fidelity = decoded.source_fidelity();
        let parts = crate::test_support::source_fidelity::given_up(fidelity);
        let [(_, retained)] = parts.as_slice() else {
            panic!("one bounded scalar failure")
        };
        assert_eq!(
            retained.offset(),
            u64::try_from(xml.find(bad).unwrap() + value_range.start).unwrap()
        );
        assert_eq!(retained.byte_len(), u64::try_from(raw_value.len()).unwrap());
        assert_eq!(retained.data(), Some(raw_value.as_bytes()));
        let errors = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(!decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "metadata.persistence-payload-unresolved"));
        assert_eq!(
            crate::test_support::source_fidelity::entry_bytes(
                fidelity,
                &crate::native::native_id("entry", "Document.xml")
            ),
            xml.as_bytes()
        );
    }
}

#[test]
fn scalar_native_and_neutral_births_are_rolled_back_with_the_failed_archive_parent() {
    let xml = document("<Property name='Bool' type='App::PropertyBool'><Bool value='true'/></Property><Property name='Float' type='App::PropertyFloat'><Float value='2.5'/></Property>", 2);
    let mut bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", xml.as_bytes()),
        RawEntry::stored("broken.bin", b"x"),
    ]);
    let offset = {
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        usize::try_from(
            cadmpeg_container::ArchiveSnapshot::new(&ctx, root)
                .unwrap()
                .entry("broken.bin")
                .unwrap()
                .header_start,
        )
        .unwrap()
    };
    cadmpeg_test_support::bytes::put_u16(&mut bytes, offset + 26, u16::MAX);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert!(decoded.ir().model.attributes.is_empty());
    let ns = decoded.ir().native.namespace("fcstd");
    assert!(ns.is_none_or(|ns| ns
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty()));
    assert!(ns.is_none_or(|ns| ns
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap()
        .is_empty()));
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [(_, retained)] = parts.as_slice() else {
        panic!("one covering archive failure")
    };
    assert_eq!(retained.offset(), 0);
    assert_eq!(retained.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(decoded.source_fidelity().retained_records().len(), 1);
    assert_eq!(retained.data(), Some(bytes.as_slice()));
}

#[test]
fn unsupported_scalar_shapes_and_dynamic_fallbacks_retain_the_generic_native_meaning() {
    for (type_name, tag) in [
        ("App::PropertyBool", "Bool"),
        ("App::PropertyFloat", "Float"),
    ] {
        for (raw, suffix, dynamic) in [
            ("1", "retained text", ""),
            ("1", "", " group='Custom' ro='TrUe' hide='unknown'"),
        ] {
            let xml = document(&format!("<Property name='Input' type='{type_name}'{dynamic}><{tag} value='{raw}'>{suffix}</{tag}></Property>"), 1);
            let dom = roxmltree::Document::parse(&xml).unwrap();
            let pure =
                crate::persistence::parse_document(&dom, crate::persistence::Vocabulary::Objects)
                    .unwrap();
            let decoded = crate::FcstdCodec
                .decode(&mut Cursor::new(archive(&xml)), &DecodeOptions::default())
                .unwrap();
            let properties = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::PropertyRecord>("properties")
                .unwrap();
            assert_eq!(properties, pure.properties);
            assert!(
                crate::test_support::source_fidelity::given_up(decoded.source_fidelity())
                    .is_empty()
            );
        }
    }
}
