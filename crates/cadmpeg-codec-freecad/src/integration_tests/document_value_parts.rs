// SPDX-License-Identifier: Apache-2.0
//! A value failure leaves its independently read property declaration held.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

mod constraints;
mod families;

fn document(properties: &str) -> String {
    format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='2'>{properties}</Properties><Objects Count='0'/><ObjectData Count='0'/></Document>")
}

#[test]
fn malformed_value_keeps_its_property_header_and_exact_child_home_with_a_readable_peer() {
    for bad in ["<Integer/>", "<Integer value='9223372036854775808'/>"] {
        let header = format!("<Property name='Bad' type='App::PropertyIntegerConstraint' status='7' group='Custom' doc='A&amp;B' ro='true'>{bad}</Property>");
        let good = "<Property name='Good' type='App::PropertyIntegerConstraint'><Integer value='17' extra='held'/></Property>";
        let xml = document(&format!("{header}{good}"));
        let bytes = zip_fixture::archive(&[
            RawEntry::stored("Document.xml", xml.as_bytes()),
            RawEntry::stored("peer.bin", b"independent"),
        ]);
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
        assert_eq!(properties.len(), 2);
        let bad_property = properties
            .iter()
            .find(|property| property.name == "Bad")
            .unwrap();
        assert!(bad_property.body.is_none());
        assert_eq!(bad_property.type_name, "App::PropertyIntegerConstraint");
        assert_eq!(bad_property.status, Some(7));
        let dynamic = bad_property.dynamic.as_ref().unwrap();
        assert_eq!(dynamic.group, "Custom");
        assert_eq!(dynamic.documentation.as_deref(), Some("A&B"));
        assert_eq!(dynamic.read_only, Some(true));
        let good_property = properties
            .iter()
            .find(|property| property.name == "Good")
            .unwrap();
        assert_eq!(good_property.values()[0].attributes["value"], "17");
        assert_eq!(good_property.values()[0].attributes["extra"], "held");
        assert!(!decoded
            .ir()
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.name == "Bad"));
        let good_attribute = decoded
            .ir()
            .model
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Good")
            .unwrap();
        assert_eq!(good_attribute.values, [AttributeValue::Integer(17)]);
        assert_eq!(
            good_attribute.native_ref.as_deref(),
            Some(good_property.id.as_str())
        );
        let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
        let [unknown] = unknowns.as_slice() else {
            panic!("one failed child")
        };
        assert_eq!(unknown.links.len(), 1);
        assert_eq!(unknown.links[0].as_str(), bad_property.id);
        let fidelity = decoded.source_fidelity();
        let part = &fidelity.parts()[&unknown.id];
        assert_eq!(part.level().as_str(), "freecad.property_value");
        assert_eq!(part.state(), &PartState::GivenUp {});
        assert_eq!(
            part.offset(),
            u64::try_from(xml.find(bad).unwrap()).unwrap()
        );
        assert_eq!(part.byte_len(), u64::try_from(bad.len()).unwrap());
        assert_eq!(
            fidelity
                .retained_record(unknown.id.as_str())
                .unwrap()
                .data(),
            Some(bad.as_bytes())
        );
        let parent = &fidelity.parts()[part.parent().unwrap()];
        assert_eq!(parent.level().as_str(), "freecad.property");
        assert_eq!(parent.state(), &PartState::Decoded {});
        assert!(fidelity
            .unrepresented()
            .contains(&cadmpeg_ir::ArenaClass::Design));
        let logical = fidelity.freecad_ledgers().values().flat_map(|ledgers| &ledgers.logical_ledger)
            .find(|span| matches!(&span.classification, cadmpeg_ir::source_fidelity::freecad::LogicalClassification::NamedOpaque { owner } if owner == &bad_property.id)).unwrap();
        assert_eq!(
            logical.span.start(),
            u64::try_from(xml.find(&header).unwrap()).unwrap()
        );
        assert_eq!(
            logical.span.end(),
            u64::try_from(xml.find(&header).unwrap() + header.len()).unwrap()
        );
        let layout = &fidelity.record_layout()[bad_property.id.as_str()];
        assert!(layout.payloads().is_empty());
        assert_eq!(layout.byte_len(), u64::try_from(header.len()).unwrap());
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
        let original = crate::source_archive::OriginalArchive::new(&ctx, source, fidelity).unwrap();
        assert_eq!(
            original.open(&ctx, "Document.xml").unwrap().window(),
            xml.as_bytes()
        );
        let reader = crate::property_reader::PropertyReader::typed(&ctx);
        assert!(reader.property_xml(bad_property).is_err());
        let mut shifted = serde_json::to_value(fidelity).unwrap();
        for field in ["parts", "record_layout", "retained_records"] {
            shifted[field][unknown.id.as_str()]["offset"] = serde_json::json!(part.offset() + 1);
        }
        for range in shifted["parts"][unknown.id.as_str()]["framing_bytes"]
            .as_array_mut()
            .unwrap()
        {
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
fn failed_value_reads_rollback_but_header_and_changed_peer_reads_survive_nonzero_windows() {
    for (raw, expected) in [("-&#55;", -7), ("8", 8)] {
        let bad = "<Integer value='9223372036854775808' extra='unadmitted'/>";
        let good = format!("<Property name='Good' type='App::PropertyIntegerConstraint'><Integer value='{raw}' extra='kept'/></Property>");
        let xml = document(&format!("<Property name='Bad' type='App::PropertyIntegerConstraint' group='Custom'>{bad}</Property>{good}"));
        let archive = zip_fixture::archive(&[RawEntry::stored("Document.xml", xml.as_bytes())]);
        let prefix = b"outside";
        let mut bytes = prefix.to_vec();
        bytes.extend(&archive);
        bytes.extend(b"after");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
        let source = root
            .child(prefix.len(), prefix.len() + archive.len())
            .unwrap();
        let (scan, output) = crate::container::scan_for_decode(&ctx, source).unwrap();
        let origin = ctx.capture_part_extent(scan.data["Document.xml"]).unwrap();
        let properties = output
            .native()
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap();
        let bad_property = properties
            .iter()
            .find(|property| property.name == "Bad")
            .unwrap();
        assert!(bad_property.body.is_none());
        let good_property = properties
            .iter()
            .find(|property| property.name == "Good")
            .unwrap();
        assert_eq!(
            good_property.values()[0].attributes["value"],
            expected.to_string()
        );
        assert_eq!(
            output
                .model()
                .attributes
                .iter()
                .find(|attribute| attribute.name == "Good")
                .unwrap()
                .values,
            [AttributeValue::Integer(expected)]
        );
        let captures = ctx.take_record_captures().unwrap();
        let bad_capture = captures
            .iter()
            .find(|capture| capture.id() == bad_property.id)
            .unwrap();
        let child_start = origin.offset() + u64::try_from(xml.find(bad).unwrap()).unwrap();
        let child_end = child_start + u64::try_from(bad.len()).unwrap();
        assert!(bad_capture.reads()[bad_capture.stream()]
            .iter()
            .all(|read| read.end <= child_start || read.start >= child_end));
        let mut from = 0;
        let expected_bad = ["Bad", "App::PropertyIntegerConstraint", "Custom"].map(|text| {
            let at = from + xml[from..].find(text).unwrap();
            from = at + text.len();
            origin.offset() + u64::try_from(at).unwrap()
                ..origin.offset() + u64::try_from(at + text.len()).unwrap()
        });
        assert_eq!(bad_capture.reads()[bad_capture.stream()], expected_bad);
        let good_capture = captures
            .iter()
            .find(|capture| capture.id() == good_property.id)
            .unwrap();
        let base = xml.find(&good).unwrap();
        let mut from = 0;
        let expected_good = [
            "Good",
            "App::PropertyIntegerConstraint",
            "value",
            raw,
            "extra",
            "kept",
        ]
        .map(|text| {
            let at = from + good[from..].find(text).unwrap();
            from = at + text.len();
            origin.offset() + u64::try_from(base + at).unwrap()
                ..origin.offset() + u64::try_from(base + at + text.len()).unwrap()
        });
        assert_eq!(good_capture.reads()[good_capture.stream()], expected_good);
    }
}
