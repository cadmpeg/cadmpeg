// SPDX-License-Identifier: Apache-2.0
//! Document integer values share actual property scopes with readable peers.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::source_fidelity::parts::PartState;
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
        let (scan, output) = crate::container::scan_for_decode(&ctx, source).unwrap();
        let origin = ctx.capture_part_extent(scan.data["Document.xml"]).unwrap();
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
        let captures = ctx.take_record_captures().unwrap();
        let capture = captures
            .iter()
            .find(|capture| capture.id() == kept.id)
            .unwrap();
        assert_eq!(capture.stream(), origin.stream());
        assert_eq!(
            capture.offset(),
            origin.offset() + u64::try_from(xml.find(&property).unwrap()).unwrap()
        );
        assert_eq!(capture.byte_len(), u64::try_from(property.len()).unwrap());
        let mut from = 0;
        let expected_reads = [
            "Count",
            "App::PropertyInteger",
            "value",
            raw,
            "extra",
            "opaque",
        ]
        .map(|text| {
            let at = from + property[from..].find(text).unwrap();
            from = at + text.len();
            let start = origin.offset() + u64::try_from(xml.find(&property).unwrap() + at).unwrap();
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
        let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
        let [unknown] = unknowns.as_slice() else { panic!("one failed integer") };
        let fidelity = decoded.source_fidelity();
        let part = &fidelity.parts()[&unknown.id];
        assert_eq!(part.level().as_str(), "freecad.property_value");
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert_eq!(part.offset(), u64::try_from(xml.find(bad).unwrap() + value_range.start).unwrap());
        assert_eq!(part.byte_len(), u64::try_from(raw_value.len()).unwrap());
        assert_eq!(fidelity.retained_record(unknown.id.as_str()).unwrap().data(), Some(raw_value.as_bytes()));
        assert_eq!(fidelity.parts()[part.parent().unwrap()].state(), &PartState::Decoded {});
        assert_eq!(fidelity.parts()[part.parent().unwrap()].level().as_str(), "freecad.property");
        let bad_id = crate::native::native_child_id("property", &crate::native::native_id("document", "0"), "Bad");
        assert_eq!(unknown.links.len(), 1);
        assert_eq!(unknown.links[0].as_str(), bad_id);
        assert!(fidelity.record_layout()[bad_id.as_str()].payloads().is_empty());
        assert_eq!(fidelity.record_layout()[bad_id.as_str()].byte_len(), u64::try_from(bad.len()).unwrap());
        let errors = decoded.report().losses.iter().filter(|loss| loss.code.local_code() == "record.unreadable").collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(!decoded.report().losses.iter().any(|loss| loss.code.local_code() == "metadata.persistence-payload-unresolved"));
        assert!(ns.arena_as::<crate::native::EntryRecord>("entries").unwrap().iter().any(|entry| entry.name() == "peer.bin"));
        let arena = DecodeArena::new();
        let (ctx, source) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let original = crate::source_archive::OriginalArchive::new(&ctx, source, fidelity).unwrap();
        assert_eq!(original.open(&ctx, "Document.xml").unwrap().window(), xml.as_bytes());
        let mut shifted = serde_json::to_value(fidelity).unwrap();
        for field in ["parts", "record_layout", "retained_records"] {
            shifted[field][unknown.id.as_str()]["offset"] = serde_json::json!(part.offset() + 1);
        }
        for range in shifted["parts"][unknown.id.as_str()]["framing_bytes"].as_array_mut().unwrap() {
            let start = range["start"].as_u64().unwrap();
            let end = range["end"].as_u64().unwrap();
            range["start"] = serde_json::json!(start + 1);
            range["end"] = serde_json::json!(end + 1);
        }
        let shifted: cadmpeg_ir::SourceFidelity = serde_json::from_value(shifted).unwrap();
        assert!(crate::source_archive::OriginalArchive::new(&ctx, source, &shifted).is_err());
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
        assert!(!decoded
            .source_fidelity()
            .parts()
            .values()
            .any(|part| part.level().as_str() == "freecad.property"));
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
