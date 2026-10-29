// SPDX-License-Identifier: Apache-2.0
//! Document integer values share actual property scopes with readable peers.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

fn document(properties: &str, count: usize) -> String {
    format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='{count}'>{properties}</Properties><Objects Count='0'/><ObjectData Count='0'/></Document>")
}

#[test]
fn integer_values_and_extra_fields_are_born_from_exact_nonzero_source_ranges() {
    for (raw, expected) in [("-&#55;", -7), ("8", 8)] {
        let property = format!("<Property name='Count' type='App::PropertyInteger' Vendor='unheld'><Integer value='{raw}' extra='opaque'/></Property>");
        let xml = document(&property, 1);
        let archive = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
        let prefix = b"outside archive";
        let mut bytes = prefix.to_vec();
        bytes.extend(&archive);
        bytes.extend(b"outside suffix");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root
            .child(prefix.len(), prefix.len() + archive.len())
            .unwrap();
        assert!(source.start() > 0);
        let (_, output) = crate::container::scan_for_decode(&ctx, source).unwrap();
        let properties = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let [kept] = properties.as_slice() else {
            panic!("one staged integer")
        };
        assert_eq!(kept.type_name, "App::PropertyInteger");
        assert_eq!(kept.values()[0].attributes["value"], expected.to_string());
        assert_eq!(kept.values()[0].attributes["extra"], "opaque");
        let [attribute] = output.model().attributes.as_slice() else {
            panic!("one staged attribute")
        };
        assert_eq!(attribute.values, [Some(AttributeValue::Integer(expected))]);
        assert_eq!(attribute.native_ref.as_deref(), Some(kept.id.as_str()));
    }
}

#[test]
fn invalid_integer_parts_lose_no_readable_native_or_neutral_peer() {
    let good = "<Property name='Good' type='App::PropertyInteger'><Integer value='17'/></Property>";
    let text = "<Property name='Note' type='App::PropertyString'><String value='peer'/></Property>";
    let baseline_xml = document(&format!("<Property name='Bad' type='App::PropertyInteger'><Integer value='1'/></Property>{good}{text}"), 3);
    let baseline = crate::FcstdCodec
        .decode(
            &mut Cursor::new(zip_fixture::archive(&[RawEntry::stored(
                "Document.xml",
                baseline_xml.as_bytes(),
            )])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let baseline_properties = baseline
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    for bad in [
        "<Property name='Bad' type='App::PropertyInteger'><Integer/></Property>",
        "<Property name='Bad' type='App::PropertyInteger'><Integer value='9223372036854775808'/></Property>",
    ] {
        let bad_dom = roxmltree::Document::parse(bad).unwrap();
        let value_range = bad_dom.root_element().first_element_child().unwrap().range();
        let raw_value = &bad[value_range.clone()];
        let xml = document(&format!("{bad}{good}{text}"), 3);
        let bytes = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes()), RawEntry::stored("peer.bin", b"independent")]);
        let decoded = crate::FcstdCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default()).unwrap();
        let ns = decoded.ir().native.namespace("fcstd").unwrap();
        let properties = ns.arena_as::<crate::native::PropertyRecord>("properties").unwrap();
        assert_eq!(properties.len(), 3);
        for name in ["Good", "Note"] {
            let kept = properties.iter().find(|property| property.name == name).unwrap();
            let original = baseline_properties.iter().find(|property| property.name == name).unwrap();
            assert_eq!(kept, original);
            let attribute = decoded.ir().model.attributes.iter().find(|attribute| attribute.name == name).unwrap();
            assert_eq!(attribute, baseline.ir().model.attributes.iter().find(|attribute| attribute.name == name).unwrap());
        }
        let bad_property = properties.iter().find(|property| property.name == "Bad").unwrap();
        assert!(bad_property.body.is_none());
        assert_eq!(bad_property.type_name, "App::PropertyInteger");
        let fidelity = decoded.source_fidelity();
        let parts = crate::test_support::source_fidelity::given_up(fidelity);
        let [(_, retained)] = parts.as_slice() else { panic!("one failed integer") };
        assert_eq!(retained.offset(), u64::try_from(xml.find(bad).unwrap() + value_range.start).unwrap());
        assert_eq!(retained.byte_len(), u64::try_from(raw_value.len()).unwrap());
        assert_eq!(retained.data(), Some(raw_value.as_bytes()));
        let errors = decoded.report().losses.iter().filter(|loss| loss.code.local_code() == "record.unreadable").collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(!decoded.report().losses.iter().any(|loss| loss.code.local_code() == "metadata.persistence-payload-unresolved"));
        assert!(ns.arena_as::<crate::native::EntryRecord>("entries").unwrap().iter().any(|entry| entry.name() == "peer.bin"));
        assert_eq!(crate::test_support::source_fidelity::entry_bytes(fidelity, &crate::native::native_id("entry", "Document.xml")), xml.as_bytes());
    }
}

#[test]
fn integer_text_and_unrecognized_dynamic_flags_keep_the_unchanged_generic_producer() {
    for extra in [
        "<Integer value='7'>retained text</Integer>",
        "<Integer value='7'/>",
    ] {
        let dynamic = if extra.contains("text") {
            ""
        } else {
            " group='Custom' ro='TrUe' hide='unknown'"
        };
        let xml = document(
            &format!(
                "<Property name='Count' type='App::PropertyInteger'{dynamic}>{extra}</Property>"
            ),
            1,
        );
        let dom = roxmltree::Document::parse(&xml).unwrap();
        let pure =
            crate::persistence::parse_document(&dom, crate::persistence::Vocabulary::Objects)
                .unwrap();
        let bytes = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
        let decoded = crate::FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
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
            crate::test_support::source_fidelity::given_up(decoded.source_fidelity()).is_empty()
        );
        assert_eq!(
            decoded
                .ir()
                .model
                .attributes
                .iter()
                .find(|attribute| attribute.name == "Count")
                .unwrap()
                .values,
            [Some(AttributeValue::Integer(7))]
        );
    }
}
