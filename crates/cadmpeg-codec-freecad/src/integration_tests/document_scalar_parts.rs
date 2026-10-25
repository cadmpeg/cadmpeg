// SPDX-License-Identifier: Apache-2.0
//! Document Bool and Float values are produced within actual source-owned parts.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::source_fidelity::parts::PartState;
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
        let origin = ctx.capture_part_extent(scan.data["Document.xml"]).unwrap();
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
            &crate::container::admit_document(&ctx, scan.data["Document.xml"].window())
                .expect("valid source XML"),
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
        assert_eq!(attribute.values, [expected]);
        assert_eq!(attribute.native_ref.as_deref(), Some(kept.id.as_str()));
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == kept.id)
            .unwrap();
        assert_eq!(capture.stream(), origin.stream());
        let start = xml.find(&property).unwrap();
        assert_eq!(
            capture.offset(),
            origin.offset() + u64::try_from(start).unwrap()
        );
        assert_eq!(capture.byte_len(), u64::try_from(property.len()).unwrap());
        let mut from = 0;
        let expected_reads = [
            "name",
            "Input",
            "type",
            type_name,
            tag,
            "value",
            raw,
            "extra",
            "native-only",
        ]
        .map(|text| {
            let at = from + property[from..].find(text).unwrap();
            from = at + text.len();
            let start = origin.offset() + u64::try_from(start + at).unwrap();
            start..start + u64::try_from(text.len()).unwrap()
        });
        assert_eq!(capture.reads()[capture.stream()], expected_reads);
        let unheld = origin.offset() + u64::try_from(xml.find("unheld").unwrap()).unwrap();
        assert!(capture.reads()[capture.stream()]
            .iter()
            .all(|read| read.end <= unheld || unheld + 6 <= read.start));
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
    for property in &properties {
        let layout = &decoded.source_fidelity().record_layout()[property.id.as_str()];
        let reads = &layout.reads()[layout.stream()];
        for (offset, _) in changed.match_indices("other") {
            let start = u64::try_from(offset).unwrap();
            let end = start + u64::try_from("other".len()).unwrap();
            assert!(reads
                .iter()
                .all(|read| read.end <= start || end <= read.start));
        }
    }
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
        let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
        let [unknown] = unknowns.as_slice() else {
            panic!("one bounded scalar failure")
        };
        let fidelity = decoded.source_fidelity();
        let part = &fidelity.parts()[&unknown.id];
        assert_eq!(part.level().as_str(), "freecad.property_value");
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert_eq!(
            part.offset(),
            u64::try_from(xml.find(bad).unwrap() + value_range.start).unwrap()
        );
        assert_eq!(part.byte_len(), u64::try_from(raw_value.len()).unwrap());
        assert_eq!(
            fidelity
                .retained_record(unknown.id.as_str())
                .unwrap()
                .data(),
            Some(raw_value.as_bytes())
        );
        assert_eq!(
            fidelity.parts()[part.parent().unwrap()].state(),
            &PartState::Decoded {}
        );
        assert_eq!(
            fidelity.parts()[part.parent().unwrap()].level().as_str(),
            "freecad.property"
        );
        let bad_id = crate::native::native_child_id(
            "property",
            &crate::native::native_id("document", "0"),
            "Bad",
        );
        assert_eq!(unknown.links.len(), 1);
        assert_eq!(unknown.links[0].as_str(), bad_id);
        assert!(fidelity.record_layout()[bad_id.as_str()]
            .payloads()
            .is_empty());
        assert_eq!(
            fidelity.record_layout()[bad_id.as_str()].byte_len(),
            u64::try_from(bad.len()).unwrap()
        );
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
        let arena = DecodeArena::new();
        let (ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        assert_eq!(
            crate::source_archive::OriginalArchive::new(&ctx, source, fidelity)
                .unwrap()
                .open(&ctx, "Document.xml")
                .unwrap()
                .window(),
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
    let ns = decoded.ir().native.namespace("fcstd").unwrap();
    assert!(ns
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty());
    assert!(ns
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap()
        .is_empty());
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one covering archive failure")
    };
    assert_eq!(
        decoded.source_fidelity().parts()[&unknown.id]
            .level()
            .as_str(),
        "zip.archive"
    );
    for name in ["Bool", "Float"] {
        let id = crate::native::native_child_id(
            "property",
            &crate::native::native_id("document", "0"),
            name,
        );
        assert!(!decoded.source_fidelity().record_layout().contains_key(&id));
    }
    assert!(decoded.source_fidelity().parts().values().all(|part| part
        .level()
        .as_str()
        .split_once('.')
        .unwrap()
        .0
        != "freecad"));
    assert_eq!(decoded.source_fidelity().retained_records().len(), 1);
    assert_eq!(
        crate::source_archive::image(decoded.source_fidelity()).unwrap(),
        bytes
    );
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
            crate::test_support::source_fidelity::assert_single_property_part(&decoded, &xml);
            if !dynamic.is_empty() {
                let layout = &decoded.source_fidelity().record_layout()[properties[0].id.as_str()];
                let reads = &layout.reads()[layout.stream()];
                let range = |text: &str| {
                    let start = u64::try_from(xml.find(text).unwrap()).unwrap();
                    start..start + u64::try_from(text.len()).unwrap()
                };
                assert!(reads.contains(&range("TrUe")));
                let unknown = range("unknown");
                assert!(reads
                    .iter()
                    .all(|read| read.end <= unknown.start || unknown.end <= read.start));
            }
        }
    }
}
