// SPDX-License-Identifier: Apache-2.0
//! What a write leaves out, how the written file still reads back, and the
//! one report line that names each omission.

use super::patching::{
    archive_payloads, assert_unheld_entry_reported, export_messages, native_arena, payload_spans,
    properties_naming, replace_arena, write_and_read_back,
};
use crate::native::{EntryRecord, PropertyRecord, RetainedXml};
use crate::test_support::test_archive::{archive_entries, CORE_DESIGN_PRODUCT};
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn decode(bytes: &[u8]) -> cadmpeg_ir::CadIr {
    FcstdCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .expect("decode source")
        .ir()
        .clone()
}

/// Removes the entry record of the payload of the first stored shape, and
/// returns the entry name and the properties that name it.
fn remove_shape_entry(ir: &mut cadmpeg_ir::CadIr) -> (String, Vec<PropertyRecord>) {
    let name = native_arena::<PropertyRecord>(ir, "properties")
        .into_iter()
        .find(|property| property.type_name == "Part::PropertyPartShape")
        .and_then(|property| property.side_entries().first().cloned())
        .expect("a stored shape names its payload entry");
    let mut entries = native_arena::<EntryRecord>(ir, "entries");
    entries.retain(|entry| entry.name() != name);
    replace_arena(ir, "entries", &entries);
    let naming = properties_naming(ir, &name);
    (name, naming)
}

/// The retained span of each property in `ids` moved one byte later, so the
/// span no longer holds the property's stored text.
fn displace_spans(ir: &mut cadmpeg_ir::CadIr, ids: &[String]) {
    let mut properties = native_arena::<PropertyRecord>(ir, "properties");
    for property in properties
        .iter_mut()
        .filter(|property| ids.contains(&property.id))
    {
        property.xml =
            RetainedXml::from_text(property.xml.text().to_owned(), property.xml.start() + 1)
                .expect("displaced span");
    }
    replace_arena(ir, "properties", &properties);
}

/// A removed payload entry whose property the patch cannot write from its
/// record, because its span no longer holds its stored text, is written
/// from the stored record with an empty file name, so the file reads back,
/// and no edit is reported dropped.
#[test]
fn a_stored_record_empties_the_file_name_of_a_removed_entry() {
    let mut ir = decode(CORE_DESIGN_PRODUCT);
    let (name, naming) = remove_shape_entry(&mut ir);
    let ids = naming
        .iter()
        .map(|property| property.id.clone())
        .collect::<Vec<_>>();
    displace_spans(&mut ir, &ids);

    let (report, bytes, round_trip) = write_and_read_back(&ir);
    assert!(!archive_payloads(&bytes).contains_key(&name));
    assert_unheld_entry_reported(&report, round_trip.ir(), &naming, &name);
    assert_eq!(export_messages(&report, "edit.dropped"), Vec::<&str>::new());
}

/// A removed optional entry whose property the patch cannot write from its
/// record, because its span no longer holds its stored text, is written from
/// the stored record with an empty file name, and its object is marked
/// `Touched`. The written file and the one `export:native.records-omitted`
/// line agree that the property names no entry; no edit is reported
/// dropped.
#[test]
fn a_property_the_patch_cannot_write_names_no_removed_optional_entry() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::FeaturePython" name="Holder"/></Objects><ObjectData Count="1"><Object name="Holder"><Properties Count="1"><Property name="Payload" type="App::PropertyFileIncluded"><FileIncluded file="Payload.bin"/></Property></Properties></Object></ObjectData></Document>"#;
    let mut ir = decode(&archive_entries(&[
        ("Document.xml", xml.as_bytes()),
        ("Payload.bin", b"payload"),
    ]));
    let mut entries = native_arena::<EntryRecord>(&ir, "entries");
    entries.retain(|entry| entry.name() != "Payload.bin");
    replace_arena(&mut ir, "entries", &entries);
    let naming = properties_naming(&ir, "Payload.bin");
    let [payload] = naming.as_slice() else {
        panic!("one property names Payload.bin, got {naming:?}");
    };
    displace_spans(&mut ir, std::slice::from_ref(&payload.id));

    let (report, bytes, round_trip) = write_and_read_back(&ir);
    assert!(!archive_payloads(&bytes).contains_key("Payload.bin"));
    assert_unheld_entry_reported(&report, round_trip.ir(), &naming, "Payload.bin");
    let written = native_arena::<PropertyRecord>(round_trip.ir(), "properties")
        .into_iter()
        .find(|property| property.id == payload.id)
        .expect("Payload written");
    assert_eq!(
        written.values()[0]
            .attributes
            .get("file")
            .map(String::as_str),
        Some("")
    );
    assert!(!round_trip
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code.local_code() == "metadata.persistence-side-entry-unresolved" }));
    let omitted = export_messages(&report, "native.records-omitted");
    let [message] = omitted.as_slice() else {
        panic!("expected one omitted line, naming the property, got {omitted:?}");
    };
    assert!(
        message.contains("each written property names no entry")
            && message.ends_with(payload.id.as_str()),
        "{message}"
    );
    assert_eq!(export_messages(&report, "edit.dropped"), Vec::<&str>::new());
}

/// Removes the entry record of `Payload.bin` from the decode of `xml`, whose
/// `Payload` property names it, and writes the document: the written
/// `Document.xml`, the report, and the round trip, with the property.
fn write_without_payload(
    xml: &str,
) -> (
    cadmpeg_ir::report::export::ExportReport,
    String,
    cadmpeg_ir::CadIr,
    PropertyRecord,
) {
    let mut ir = decode(&archive_entries(&[
        ("Document.xml", xml.as_bytes()),
        ("Payload.bin", b"payload"),
    ]));
    let mut entries = native_arena::<EntryRecord>(&ir, "entries");
    entries.retain(|entry| entry.name() != "Payload.bin");
    replace_arena(&mut ir, "entries", &entries);
    let naming = properties_naming(&ir, "Payload.bin");
    let [payload] = naming.as_slice() else {
        panic!("one property names Payload.bin, got {naming:?}");
    };
    let (report, bytes, round_trip) = write_and_read_back(&ir);
    let payloads = archive_payloads(&bytes);
    assert!(!payloads.contains_key("Payload.bin"));
    let document = String::from_utf8(payloads["Document.xml"].clone()).expect("UTF-8 Document.xml");
    (report, document, round_trip.ir().clone(), payload.clone())
}

/// A property whose `file` attribute on a nested value element names a
/// removed entry, which the patch cannot write empty from the record or from
/// the stored record, is left out of the written file, its container's count
/// is lowered, and its object is marked `Touched` (W1). One
/// `export:native.records-omitted` line names the property; no edit is
/// reported dropped, and no ledger span of the entry is named.
#[test]
fn a_property_the_patch_cannot_empty_is_left_out() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::FeaturePython" name="Holder"/></Objects><ObjectData Count="1"><Object name="Holder"><Properties Count="1"><Property name="Payload" type="App::PropertyFileIncluded"><FileIncluded file="Payload.bin"><Note/></FileIncluded></Property></Properties></Object></ObjectData></Document>"#;
    let (report, document, round_trip, payload) = write_without_payload(xml);
    assert!(
        document.contains(r#"<Properties Count="0">"#) && !document.contains("Payload"),
        "{document}"
    );
    assert!(native_arena::<PropertyRecord>(&round_trip, "properties")
        .iter()
        .all(|property| property.id != payload.id));
    let holder = native_arena::<crate::native::ObjectRecord>(&round_trip, "objects")
        .into_iter()
        .find(|object| *object.id() == payload.owner)
        .expect("Holder written");
    assert_eq!(
        holder.attributes.get("Touched").map(String::as_str),
        Some("1")
    );
    let omitted = export_messages(&report, "native.records-omitted");
    let [message] = omitted.as_slice() else {
        panic!("expected one omitted line, got {omitted:?}");
    };
    assert!(
        message.contains("arena `properties`") && message.ends_with(&format!(": {}", payload.id)),
        "{message}"
    );
    assert_eq!(export_messages(&report, "edit.dropped"), Vec::<&str>::new());
}

/// Where the patch cannot remove that property's element, because its
/// container declares no count, the property is written as stored and still
/// names the entry. The written file holds the property and lacks the
/// entry's member, so one `export:native.records-omitted` line names the
/// ledger span of that member; no edit is reported dropped.
#[test]
fn a_property_the_patch_cannot_remove_is_written_as_stored() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::FeaturePython" name="Holder"/></Objects><ObjectData Count="1"><Object name="Holder"><Properties><Property name="Payload" type="App::PropertyFileIncluded"><FileIncluded file="Payload.bin"><Note/></FileIncluded></Property></Properties></Object></ObjectData></Document>"#;
    let (report, _, round_trip, payload) = write_without_payload(xml);
    let written = native_arena::<PropertyRecord>(&round_trip, "properties")
        .into_iter()
        .find(|property| property.id == payload.id)
        .expect("Payload written");
    assert_eq!(
        written.values()[0]
            .attributes
            .get("file")
            .map(String::as_str),
        Some("Payload.bin")
    );
    let omitted = export_messages(&report, "native.records-omitted");
    let [message] = omitted.as_slice() else {
        panic!("expected one omitted line, got {omitted:?}");
    };
    assert!(
        message.contains(
            "of `fcstd` arena `physical_ledger` whose ZIP member has no archive entry record: "
        ) && !message.contains(&payload.id),
        "{message}"
    );
    assert_eq!(export_messages(&report, "edit.dropped"), Vec::<&str>::new());
}

/// Each member the output no longer holds is named on one
/// `export:native.records-omitted` line. The property that names a member
/// accounts for it, so no ledger span of that member is named; a member that
/// no property names is named by its ledger span.
#[test]
fn a_removed_member_is_named_on_one_line() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::FeaturePython" name="Holder"/></Objects><ObjectData Count="1"><Object name="Holder"><Properties Count="1"><Property name="Payload" type="App::PropertyFileIncluded"><FileIncluded file="Payload.bin"/></Property></Properties></Object></ObjectData></Document>"#;
    let mut ir = decode(&archive_entries(&[
        ("Document.xml", xml.as_bytes()),
        ("Payload.bin", b"payload"),
        ("Extra.bin", b"extra"),
    ]));
    let (named_spans, unnamed_spans) = (
        payload_spans(&ir, "Payload.bin"),
        payload_spans(&ir, "Extra.bin"),
    );
    assert!(!named_spans.is_empty() && !unnamed_spans.is_empty());
    let mut entries = native_arena::<EntryRecord>(&ir, "entries");
    entries.retain(|entry| entry.name() == "Document.xml");
    replace_arena(&mut ir, "entries", &entries);
    let payload = properties_naming(&ir, "Payload.bin")
        .pop()
        .expect("Payload names its entry");
    assert!(properties_naming(&ir, "Extra.bin").is_empty());

    let (report, bytes, _) = write_and_read_back(&ir);
    let payloads = archive_payloads(&bytes);
    assert!(!payloads.contains_key("Payload.bin") && !payloads.contains_key("Extra.bin"));
    let omitted = export_messages(&report, "native.records-omitted");
    assert_eq!(omitted.len(), 2, "{omitted:?}");
    let property_lines = omitted
        .iter()
        .filter(|message| message.contains(payload.id.as_str()))
        .collect::<Vec<_>>();
    let [property_line] = property_lines.as_slice() else {
        panic!("expected one line naming the property, got {omitted:?}");
    };
    assert!(property_line.contains("names no entry"), "{property_line}");
    for span in &named_spans {
        assert!(
            omitted
                .iter()
                .all(|message| !message.contains(span.as_str())),
            "{span}: {omitted:?}"
        );
    }
    let span_lines = omitted
        .iter()
        .filter(|message| {
            unnamed_spans
                .iter()
                .any(|span| message.contains(span.as_str()))
        })
        .collect::<Vec<_>>();
    let [span_line] = span_lines.as_slice() else {
        panic!("expected one line naming a span of the unnamed member, got {omitted:?}");
    };
    assert!(
        span_line.contains("has no archive entry record"),
        "{span_line}"
    );
}

/// A `Document.xml` entry edited so that no property span holds its stored
/// text, beside a removed payload entry, is written as stored. Only the
/// property whose edit the stored document does not hold is reported
/// dropped.
#[test]
fn the_stored_document_names_only_the_dropped_property_edits() {
    let mut ir = decode(CORE_DESIGN_PRODUCT);
    let mut entries = native_arena::<EntryRecord>(&ir, "entries");
    let document = entries
        .iter_mut()
        .find(|entry| entry.name() == "Document.xml")
        .expect("Document.xml entry");
    let text = std::str::from_utf8(document.data()).expect("UTF-8 Document.xml");
    let shifted = text.replacen("<Document ", "<Document  ", 1);
    assert_ne!(shifted, text);
    *document = crate::test_support::entry_record(
        document.id().to_owned(),
        document.name().to_owned(),
        document.role,
        document.referenced_by().to_vec(),
        shifted.into_bytes(),
    );
    replace_arena(&mut ir, "entries", &entries);
    let (name, _) = remove_shape_entry(&mut ir);
    let object = native_arena::<crate::native::ObjectRecord>(&ir, "objects")
        .into_iter()
        .find(|object| {
            native_arena::<PropertyRecord>(&ir, "properties")
                .iter()
                .any(|property| property.owner == *object.id() && property.name == "Label")
        })
        .expect("an object with a Label");
    FcstdCodec
        .set_property_value_attribute(
            &mut ir,
            crate::FcstdPropertyOwner::Object(object.name()),
            "Label",
            0,
            "value",
            "Renamed",
        )
        .expect("edit Label");
    let label = native_arena::<PropertyRecord>(&ir, "properties")
        .into_iter()
        .find(|property| property.owner == *object.id() && property.name == "Label")
        .expect("Label");

    let (report, bytes, round_trip) = write_and_read_back(&ir);
    assert!(!archive_payloads(&bytes).contains_key(&name));
    let written = native_arena::<PropertyRecord>(round_trip.ir(), "properties")
        .into_iter()
        .find(|property| property.id == label.id)
        .expect("Label written");
    assert_ne!(
        written.values()[0]
            .attributes
            .get("value")
            .map(String::as_str),
        Some("Renamed")
    );
    let dropped = export_messages(&report, "edit.dropped");
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert!(
        dropped[0].ends_with(&format!("(1): {}", label.id)),
        "{dropped:?}"
    );
}

/// An entry record that does not read leaves its ZIP member out. The report
/// names the record once and none of the member's ledger spans.
#[test]
fn an_unreadable_entry_is_named_once() {
    let mut ir = decode(CORE_DESIGN_PRODUCT);
    let mut entries = native_arena::<serde_json::Value>(&ir, "entries");
    let entry = entries
        .iter_mut()
        .find(|entry| entry["name"] != "Document.xml")
        .expect("side entry");
    let (stale_id, stale_name) = (
        entry["id"].as_str().expect("entry id").to_owned(),
        entry["name"].as_str().expect("entry name").to_owned(),
    );
    let data = entry["data"].as_str().expect("entry hex bytes");
    entry["data"] = serde_json::json!(format!("{data}00"));
    replace_arena(&mut ir, "entries", &entries);
    let spans = payload_spans(&ir, &stale_name);
    assert!(!spans.is_empty());

    let (report, bytes, _) = write_and_read_back(&ir);
    assert!(!archive_payloads(&bytes).contains_key(&stale_name));
    let omitted = export_messages(&report, "native.records-omitted");
    assert_eq!(
        omitted
            .iter()
            .filter(|message| message.contains(stale_id.as_str()))
            .count(),
        1,
        "{omitted:?}"
    );
    for span in &spans {
        assert!(
            omitted
                .iter()
                .all(|message| !message.contains(span.as_str())),
            "{span}: {omitted:?}"
        );
    }
}

/// A physical ledger span that does not read orders no member. Every entry
/// is still written, and the report names the span.
#[test]
fn an_unreadable_ledger_span_is_named() {
    let mut ir = decode(CORE_DESIGN_PRODUCT);
    let mut ledger = native_arena::<serde_json::Value>(&ir, "physical_ledger");
    let span = ledger
        .iter_mut()
        .find(|span| span["role"] == "local-signature")
        .expect("local signature span");
    span["end"] = span["start"].clone();
    let span_id = span["id"].as_str().expect("span id").to_owned();
    replace_arena(&mut ir, "physical_ledger", &ledger);

    let (report, bytes, _) = write_and_read_back(&ir);
    let payloads = archive_payloads(&bytes);
    for entry in native_arena::<EntryRecord>(&ir, "entries")
        .iter()
        .filter(|entry| entry.name() != "Document.xml")
    {
        assert_eq!(payloads[entry.name()], entry.data(), "{}", entry.name());
    }
    let omitted = export_messages(&report, "native.records-omitted");
    assert!(
        omitted
            .iter()
            .any(|message| message.contains(span_id.as_str())),
        "{omitted:?}"
    );
}

/// Schema 2 declares no recompute mark. A property whose payload entry the
/// archive does not hold is written with an empty file name, and the report
/// names its object stale beside the omitted property.
#[test]
fn a_schema_two_object_with_an_emptied_file_name_is_reported_stale() {
    let xml = r#"<Document SchemaVersion="2" FileVersion="1"><Features Count="1"><Feature type="App::FeaturePython" name="Holder"/></Features><FeatureData Count="1"><Feature name="Holder"><Properties Count="1"><Property name="Payload" type="App::PropertyFileIncluded"><File file="Payload.bin"/></Property></Properties></Feature></FeatureData></Document>"#;
    let mut ir = decode(&archive_entries(&[
        ("Document.xml", xml.as_bytes()),
        ("Payload.bin", b"payload"),
    ]));
    let mut entries = native_arena::<EntryRecord>(&ir, "entries");
    entries.retain(|entry| entry.name() != "Payload.bin");
    replace_arena(&mut ir, "entries", &entries);
    let payload = properties_naming(&ir, "Payload.bin")
        .pop()
        .expect("Payload names its entry");

    let (report, bytes, round_trip) = write_and_read_back(&ir);
    let written = native_arena::<PropertyRecord>(round_trip.ir(), "properties")
        .into_iter()
        .find(|property| property.id == payload.id)
        .expect("Payload written");
    assert!(written.side_entries().is_empty());
    assert_eq!(
        written.values()[0]
            .attributes
            .get("file")
            .map(String::as_str),
        Some("")
    );
    let document = String::from_utf8(archive_payloads(&bytes)["Document.xml"].clone())
        .expect("UTF-8 Document.xml");
    assert!(!document.contains("Touched"), "{document}");
    let holder = crate::native::native_id("object", "Holder");
    let stale = export_messages(&report, "evaluation.stale");
    assert_eq!(stale.len(), 1, "{stale:?}");
    assert!(stale[0].contains(holder.as_str()), "{stale:?}");
    let omitted = export_messages(&report, "native.records-omitted");
    assert!(
        omitted
            .iter()
            .any(|message| message.contains(payload.id.as_str())),
        "{omitted:?}"
    );
}
