// SPDX-License-Identifier: Apache-2.0
//! Atomic source-owned document string parsing and native/neutral birth.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::zip_fixture::{self, RawEntry};
use std::io::Cursor;

const BAD: &str =
    "<Property name='Bad' type='App::PropertyString'><String Value='bad'/></Property>";
const NAMELESS: &str =
    "<Property type='App::PropertyString'><String value='requires a property name'/></Property>";
const GOOD: &str = "<Property name='Good' type='App::PropertyString'><String value='A&amp;B' extra='opaque'/></Property>";

fn document() -> String {
    format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='4'>{NAMELESS}{BAD}{GOOD}<Property name='Other' type='App::PropertyInteger'><Integer value='17'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>")
}

#[test]
fn property_native_and_neutral_values_exist_before_later_graph_recovery() {
    let document = document();
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document.as_bytes()),
        RawEntry::stored("peer.bin", b"peer"),
    ]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (scan, output) = crate::container::scan_for_decode(&ctx, root).unwrap();
    assert_eq!(scan.data["peer.bin"].window(), b"peer");
    assert_eq!(output.model().attributes.len(), 2);
    let alias = output
        .model()
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Bad")
        .unwrap();
    assert_eq!(alias.values, [AttributeValue::String("bad".into())]);
    let attribute = output
        .model()
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Good")
        .unwrap();
    assert_eq!(attribute.name, "Good");
    assert_eq!(attribute.target, AttributeTarget::Document);
    assert_eq!(attribute.values, [AttributeValue::String("A&B".into())]);
    let good_id = crate::native::native_child_id(
        "property",
        &crate::native::native_id("document", "0"),
        "Good",
    );
    assert_eq!(attribute.native_ref.as_deref(), Some(good_id.as_str()));
    let staged = output
        .native()
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert_eq!(staged.len(), 2);
    let good = staged
        .iter()
        .find(|property| property.id == good_id)
        .unwrap();
    assert_eq!(good.values()[0].attributes["value"], "A&B");
    assert_eq!(good.values()[0].attributes["extra"], "opaque");
    let alias = staged
        .iter()
        .find(|property| property.name == "Bad")
        .unwrap();
    assert_eq!(alias.values()[0].attributes["Value"], "bad");
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let unknowns = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one malformed property")
    };
    let part = &fidelity.parts()[&unknown.id];
    assert_eq!(part.level().as_str(), "freecad.property");
    assert_eq!(part.state(), &PartState::GivenUp {});
    assert_eq!(
        part.offset(),
        u64::try_from(document.find(NAMELESS).unwrap()).unwrap()
    );
    assert_eq!(part.byte_len(), u64::try_from(NAMELESS.len()).unwrap());
    assert_eq!(
        fidelity
            .retained_record(unknown.id.as_str())
            .unwrap()
            .data(),
        Some(NAMELESS.as_bytes())
    );
    let parent = &fidelity.parts()[part.parent().unwrap()];
    assert_eq!(parent.level().as_str(), "freecad.properties");
    assert_eq!(parent.state(), &PartState::Decoded {});
    assert_eq!(
        fidelity.parts()[parent.parent().unwrap()].level().as_str(),
        "freecad.document"
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == good_id)
        .unwrap();
    let actual = capture.reads().get(capture.stream()).unwrap();
    let start = document.find(GOOD).unwrap();
    let expected = [
        "Good",
        "App::PropertyString",
        "value",
        "A&amp;B",
        "extra",
        "opaque",
    ]
    .into_iter()
    .map(|spelling| {
        let at = start + GOOD.find(spelling).unwrap();
        u64::try_from(at).unwrap()..u64::try_from(at + spelling.len()).unwrap()
    })
    .collect::<Vec<_>>();
    assert_eq!(actual, &expected);
    let bad_id = crate::native::native_child_id(
        "property",
        &crate::native::native_id("document", "0"),
        "Bad",
    );
    let alias_capture = captures
        .iter()
        .find(|capture| capture.id() == bad_id)
        .unwrap();
    let alias_at = document.find(BAD).unwrap() + BAD.find("Value").unwrap();
    assert!(alias_capture.reads()[alias_capture.stream()].contains(
        &(u64::try_from(alias_at).unwrap()..u64::try_from(alias_at + "Value".len()).unwrap())
    ));
    assert!(!captures
        .iter()
        .any(|capture| capture.stream() == part.stream()
            && capture.offset() == part.offset()
            && capture.byte_len() == part.byte_len()));
}

#[test]
fn public_decode_keeps_the_good_property_attribute_and_unrelated_integer_once() {
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document().as_bytes()),
        RawEntry::stored("peer.bin", b"peer"),
    ]);
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let attributes = &decoded.ir().model.attributes;
    let good = attributes
        .iter()
        .filter(|attribute| attribute.name == "Good")
        .collect::<Vec<_>>();
    assert_eq!(good.len(), 1);
    assert_eq!(good[0].values, [AttributeValue::String("A&B".into())]);
    let alias = attributes
        .iter()
        .filter(|attribute| attribute.name == "Bad")
        .collect::<Vec<_>>();
    assert_eq!(alias.len(), 1);
    assert_eq!(alias[0].values, [AttributeValue::String("bad".into())]);
    assert_eq!(
        attributes
            .iter()
            .filter(|attribute| attribute.name == "Other")
            .count(),
        1
    );
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert_eq!(
        properties
            .iter()
            .map(|property| property.name.as_str())
            .collect::<Vec<_>>(),
        ["Bad", "Good", "Other"]
    );
    assert!(properties
        .iter()
        .all(|property| !matches!(property.body, crate::native::PropertyBody::Unreadable(_))));
    assert_eq!(
        native
            .arena_as::<crate::native::EntryRecord>("entries")
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .count(),
        1
    );
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "metadata.persistence-payload-unresolved"));
    assert_eq!(
        crate::source_archive::image(decoded.source_fidelity()).unwrap(),
        bytes
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let original =
        crate::source_archive::OriginalArchive::new(&ctx, root, decoded.source_fidelity()).unwrap();
    assert_eq!(
        original.open(&ctx, "Document.xml").unwrap().window(),
        document().as_bytes()
    );
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    let [unknown] = unknowns.as_slice() else {
        panic!("one failed property")
    };
    let mut shifted = serde_json::to_value(decoded.source_fidelity()).unwrap();
    let offset = decoded.source_fidelity().parts()[&unknown.id].offset();
    for field in ["parts", "record_layout", "retained_records"] {
        shifted[field][unknown.id.as_str()]["offset"] = serde_json::json!(offset + 1);
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
    assert!(crate::source_archive::OriginalArchive::new(&ctx, root, &shifted).is_err());
}

#[test]
fn an_archive_parent_failure_discards_already_born_property_native_and_neutral_records() {
    let xml = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='Good' type='App::PropertyString'><String value='kept only on success'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
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
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    assert!(native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty());
    assert!(native
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
    assert!(decoded
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.property"
            && part.state()
                == &PartState::Covered {
                    by: unknown.id.clone()
                }));
    let property_id = crate::native::native_child_id(
        "property",
        &crate::native::native_id("document", "0"),
        "Good",
    );
    assert!(!decoded
        .source_fidelity()
        .record_layout()
        .contains_key(&property_id));
    assert_eq!(decoded.source_fidelity().retained_records().len(), 1);
    assert_eq!(
        crate::source_archive::image(decoded.source_fidelity()).unwrap(),
        bytes
    );
}

#[test]
fn retained_extra_bytes_change_native_values_and_have_exact_source_reads() {
    for extra in ["opaque", "changed"] {
        let xml = format!("<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='Good' type='App::PropertyString' ghost='unheld'><String value='A&amp;B' extra='{extra}'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>");
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
        let [property] = properties.as_slice() else {
            panic!("one held property")
        };
        assert_eq!(property.values()[0].attributes["extra"], extra);
        assert_eq!(
            decoded.ir().model.attributes[0].values,
            [AttributeValue::String("A&B".into())]
        );
        let layout = &decoded.source_fidelity().record_layout()[property.id.as_str()];
        let reads = &layout.reads()[layout.stream()];
        let at = xml.find(extra).unwrap();
        assert!(
            reads.contains(&(u64::try_from(at).unwrap()..u64::try_from(at + extra.len()).unwrap()))
        );
        let at = xml.find("extra=").unwrap();
        assert!(reads
            .contains(&(u64::try_from(at).unwrap()..u64::try_from(at + "extra".len()).unwrap())));
        let at = u64::try_from(xml.find("unheld").unwrap()).unwrap();
        assert!(!reads
            .iter()
            .any(|range| range.start <= at && at < range.end));
        assert!(serde_json::to_value(property)
            .unwrap()
            .get("ghost")
            .is_none());
    }
}

#[test]
fn unsupported_dynamic_flag_spellings_keep_legacy_fields_without_read_credit() {
    let xml = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='Flags' type='App::PropertyString' group='Readouts' ro='TrUe' hide='unknown'><String value='kept'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
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
    let [property] = properties.as_slice() else {
        panic!("one held property")
    };
    let crate::native::PropertyBody::Persisted {
        dynamic: Some(dynamic),
        ..
    } = &property.body
    else {
        panic!("dynamic flags held")
    };
    assert_eq!(dynamic.read_only, Some(false));
    assert_eq!(dynamic.hidden, Some(false));
    let layout = &decoded.source_fidelity().record_layout()[property.id.as_str()];
    let reads = &layout.reads()[layout.stream()];
    let expected = ["Flags", "App::PropertyString", "Readouts", "kept"].map(|value| {
        let at = xml.find(value).unwrap();
        u64::try_from(at).unwrap()..u64::try_from(at + value.len()).unwrap()
    });
    assert_eq!(reads, &expected);
    for spelling in ["TrUe", "unknown"] {
        let at = xml.find(spelling).unwrap();
        let range = u64::try_from(at).unwrap()..u64::try_from(at + spelling.len()).unwrap();
        assert!(reads
            .iter()
            .all(|read| read.end <= range.start || range.end <= read.start));
    }
    assert!(!decoded
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.property"));
}

#[test]
fn recognized_dynamic_flags_keep_their_values_inside_the_actual_property_scope() {
    let xml = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='Flags' type='App::PropertyString' group='Readouts' ro='1' hide='false'><String value='kept'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
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
    let [property] = properties.as_slice() else {
        panic!("one held property")
    };
    let crate::native::PropertyBody::Persisted {
        dynamic: Some(dynamic),
        ..
    } = &property.body
    else {
        panic!("recognized dynamic fields")
    };
    assert_eq!(dynamic.read_only, Some(true));
    assert_eq!(dynamic.hidden, Some(false));
    let layout = &decoded.source_fidelity().record_layout()[property.id.as_str()];
    let reads = &layout.reads()[layout.stream()];
    for (key, value) in [("ro", "1"), ("hide", "false")] {
        let prefix = format!("{key}='");
        let start = xml.find(&prefix).unwrap() + prefix.len();
        let range = u64::try_from(start).unwrap()..u64::try_from(start + value.len()).unwrap();
        assert!(reads.contains(&range));
    }
    assert!(decoded.source_fidelity().parts().values().any(|part| {
        part.level().as_str() == "freecad.property"
            && part.state() == &PartState::Decoded {}
            && part.stream() == layout.stream()
            && part.offset() == layout.offset()
            && part.byte_len() == layout.byte_len()
    }));
    let attribute = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Flags")
        .unwrap();
    assert_eq!(attribute.native_ref.as_deref(), Some(property.id.as_str()));
    assert_eq!(attribute.values, [AttributeValue::String("kept".into())]);
}

#[test]
fn only_the_final_retained_local_attribute_key_and_value_get_read_credit() {
    let xml = "<Document SchemaVersion='4' FileVersion='1' xmlns:a='urn:test'><Properties Count='1'><Property name='Good' type='App::PropertyString'><String value='kept' a:extra='discarded' extra='winner'/></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
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
    let [property] = properties.as_slice() else {
        panic!("one held property")
    };
    assert_eq!(property.values()[0].attributes["extra"], "winner");
    let layout = &decoded.source_fidelity().record_layout()[property.id.as_str()];
    let reads = &layout.reads()[layout.stream()];
    let at = xml.find("winner").unwrap();
    assert!(
        reads.contains(&(u64::try_from(at).unwrap()..u64::try_from(at + "winner".len()).unwrap()))
    );
    let at = u64::try_from(xml.find("discarded").unwrap()).unwrap();
    assert!(!reads
        .iter()
        .any(|range| range.start <= at && at < range.end));
}

#[test]
fn unsupported_cdata_text_stays_on_the_existing_native_producer_without_a_fake_property_part() {
    let xml = "<Document SchemaVersion='4' FileVersion='1'><Properties Count='1'><Property name='Text' type='App::PropertyString'><String value='kept'><![CDATA[actual text]]></String></Property></Properties><Objects Count='0'/><ObjectData Count='0'/></Document>";
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
    let [property] = properties.as_slice() else {
        panic!("one native property")
    };
    assert_eq!(property.values()[0].text.as_deref(), Some("actual text"));
    assert_eq!(property.values()[0].attributes["value"], "kept");
    assert!(!decoded
        .source_fidelity()
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.property"));
}
