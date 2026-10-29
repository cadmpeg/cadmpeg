// SPDX-License-Identifier: Apache-2.0
//! Atomic source-owned document string parsing and native/neutral birth.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
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
    assert_eq!(output.model().attributes.len(), 3);
    let alias = output
        .model()
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Bad")
        .unwrap();
    assert_eq!(alias.values, [Some(AttributeValue::String("bad".into()))]);
    let attribute = output
        .model()
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Good")
        .unwrap();
    assert_eq!(attribute.name, "Good");
    assert_eq!(attribute.target, AttributeTarget::Document);
    assert_eq!(
        attribute.values,
        [Some(AttributeValue::String("A&B".into()))]
    );
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
    assert_eq!(staged.len(), 3);
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
    let other = staged
        .iter()
        .find(|property| property.name == "Other")
        .unwrap();
    assert_eq!(other.type_name, "App::PropertyInteger");
    assert_eq!(other.values()[0].attributes["value"], "17");
    let other_attribute = output
        .model()
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Other")
        .unwrap();
    assert_eq!(other_attribute.values, [Some(AttributeValue::Integer(17))]);
    assert_eq!(
        other_attribute.native_ref.as_deref(),
        Some(other.id.as_str())
    );
    let (_, _, fidelity, losses) = output.into_parts();
    let parts = crate::test_support::source_fidelity::given_up(&fidelity);
    let [(_, retained)] = parts.as_slice() else {
        panic!("one malformed property")
    };
    assert_eq!(
        retained.offset(),
        u64::try_from(document.find(NAMELESS).unwrap()).unwrap()
    );
    assert_eq!(retained.byte_len(), u64::try_from(NAMELESS.len()).unwrap());
    assert_eq!(retained.data(), Some(NAMELESS.as_bytes()));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
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
    assert_eq!(good[0].values, [Some(AttributeValue::String("A&B".into()))]);
    let alias = attributes
        .iter()
        .filter(|attribute| attribute.name == "Bad")
        .collect::<Vec<_>>();
    assert_eq!(alias.len(), 1);
    assert_eq!(
        alias[0].values,
        [Some(AttributeValue::String("bad".into()))]
    );
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
    assert!(properties.iter().all(|property| !matches!(
        property.body,
        Some(crate::native::PropertyBody::Unreadable(_))
    )));
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
    assert_eq!(
        crate::test_support::source_fidelity::entry_bytes(
            decoded.source_fidelity(),
            &crate::native::native_id("entry", "Document.xml")
        ),
        document().as_bytes()
    );
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [_] = parts.as_slice() else {
        panic!("one failed property")
    };
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
    let native = decoded.ir().native.namespace("fcstd");
    assert!(native.is_none_or(|native| native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty()));
    assert!(native.is_none_or(|native| native
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
            [Some(AttributeValue::String("A&B".into()))]
        );
        assert!(serde_json::to_value(property)
            .unwrap()
            .get("ghost")
            .is_none());
    }
}

#[test]
fn unrecognized_dynamic_flag_spelling_is_absent_without_read_credit() {
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
    assert!(matches!(
        &property.body,
        Some(crate::native::PropertyBody::Persisted { .. })
    ));
    let Some(dynamic) = &property.dynamic else {
        panic!("dynamic flags held")
    };
    assert_eq!(dynamic.read_only, Some(true));
    assert_eq!(dynamic.hidden, None);
    assert!(crate::test_support::source_fidelity::given_up(decoded.source_fidelity()).is_empty());
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
    assert!(matches!(
        &property.body,
        Some(crate::native::PropertyBody::Persisted { .. })
    ));
    let Some(dynamic) = &property.dynamic else {
        panic!("recognized dynamic fields")
    };
    assert_eq!(dynamic.read_only, Some(true));
    assert_eq!(dynamic.hidden, Some(false));
    let attribute = decoded
        .ir()
        .model
        .attributes
        .iter()
        .find(|attribute| attribute.name == "Flags")
        .unwrap();
    assert_eq!(attribute.native_ref.as_deref(), Some(property.id.as_str()));
    assert_eq!(
        attribute.values,
        [Some(AttributeValue::String("kept".into()))]
    );
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
    assert!(crate::test_support::source_fidelity::given_up(decoded.source_fidelity()).is_empty());
}
