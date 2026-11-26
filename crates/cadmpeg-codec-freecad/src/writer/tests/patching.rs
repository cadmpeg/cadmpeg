// SPDX-License-Identifier: Apache-2.0
//! The `Document.xml` patch and the archive repack: property edits, value
//! serialization, entry preservation, and the checks that refuse an edit no
//! retained span can carry.

use super::super::target::retained_baseline;
use crate::native::{PropertyRecord, ValueRecord};
use crate::test_support::test_archive::{archive_entries, CORE_DESIGN_PRODUCT};
use crate::writer::{entry_order, serialize_property, serialize_value};
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn unsafe_entry_names_refuse_construction() {
    assert!(crate::native::EntryRecord::new(
        "test:native:entry#unsafe".into(),
        "../Document.xml".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        Vec::new()
    )
    .is_err());
}

/// The archive keeps the source's member order: each entry at the lowest
/// local-header offset the ledger states for its name, and an entry the
/// ledger places nowhere after them, in arena order.
#[test]
fn entries_follow_the_ledger_order_and_unplaced_entries_follow_them() {
    let entry = |name: &str| {
        crate::test_support::entry_record(
            format!("test:native:entry#{name}"),
            name.into(),
            cadmpeg_core::container::ContainerRole::Auxiliary,
            Vec::new(),
            Vec::new(),
        )
    };
    let span = |id: &str, name: &str, start: u64| {
        cadmpeg_core::serde_erase::json::from_value::<crate::native::ArchiveSpan>(
            serde_json::json!({
                "id": id,
                "start": start,
                "end": start + 4,
                "role": "local-signature",
                "entry": name,
            }),
        )
        .expect("ledger span fixture")
    };
    let entries = [
        entry("Unplaced"),
        entry("Late"),
        entry("Early"),
        entry("Last"),
    ];
    let ledger = [
        span("test:ledger#0", "Late", 40),
        span("test:ledger#1", "Early", 10),
        span("test:ledger#2", "Late", 20),
    ];
    let names = |ledger: &[crate::native::ArchiveSpan]| {
        entry_order(&entries, ledger)
            .into_iter()
            .map(crate::native::EntryRecord::name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&ledger), ["Early", "Late", "Unplaced", "Last"]);
    assert_eq!(names(&[]), ["Unplaced", "Late", "Early", "Last"]);
}

/// The message of each `export:` loss of `report` with this local code.
pub(super) fn export_messages<'r>(
    report: &'r cadmpeg_ir::report::export::ExportReport,
    code: &str,
) -> Vec<&'r str> {
    report
        .losses
        .iter()
        .filter(|loss| loss.code.namespace() == "export" && loss.code.local_code() == code)
        .map(|loss| loss.message.as_str())
        .collect()
}

/// Writes `ir` with a request that names no target and decodes the file.
pub(super) fn write_and_read_back(
    ir: &cadmpeg_ir::CadIr,
) -> (
    cadmpeg_ir::report::export::ExportReport,
    Vec<u8>,
    cadmpeg_ir::DecodeResult,
) {
    let mut bytes = Vec::new();
    let report = FcstdCodec
        .plan(EncodeInput::new(ir, None))
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the retained document patches");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("the written file decodes");
    (report, bytes, decoded)
}

pub(super) fn native_arena<T: serde::de::DeserializeOwned>(
    ir: &cadmpeg_ir::CadIr,
    arena: &str,
) -> Vec<T> {
    ir.native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<T>(arena)
        .expect("typed arena")
}

pub(super) fn replace_arena<T: serde::Serialize>(
    ir: &mut cadmpeg_ir::CadIr,
    arena: &str,
    records: &[T],
) {
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("fcstd"))
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            arena,
            records,
        )
        .expect("replace arena");
}

/// A second entry record that names an entry the archive already holds is
/// left out, and the report names it.
#[test]
fn a_repeated_entry_name_is_written_once_and_reported() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let mut ir = decoded.ir().clone();
    let mut entries = native_arena::<crate::native::EntryRecord>(&ir, "entries");
    let source = entries
        .iter()
        .find(|entry| entry.name() != "Document.xml")
        .expect("side entry")
        .clone();
    entries.push(crate::test_support::entry_record(
        "fcstd:test:entry#repeated".into(),
        source.name().to_owned(),
        source.role,
        Vec::new(),
        b"repeated".to_vec(),
    ));
    replace_arena(&mut ir, "entries", &entries);

    let (report, bytes, _) = write_and_read_back(&ir);
    let payloads = archive_payloads(&bytes);
    assert_eq!(payloads[source.name()], source.data());
    assert_eq!(payloads.len(), entries.len() - 1);
    let omitted = export_messages(&report, "native.records-omitted");
    assert!(
        omitted
            .iter()
            .any(|message| message.contains("fcstd:test:entry#repeated")),
        "{omitted:?}"
    );
}

/// An entry the document no longer holds is left out of the archive. The
/// report names each property that names it, and no ledger span of its ZIP
/// member, since those properties account for the member.
#[test]
fn a_removed_entry_is_left_out_and_its_member_is_reported() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let mut ir = decoded.ir().clone();
    let mut entries = native_arena::<crate::native::EntryRecord>(&ir, "entries");
    let index = entries
        .iter()
        .position(|entry| entry.name() != "Document.xml")
        .expect("side entry");
    let removed = entries.remove(index);
    let naming = properties_naming(&ir, removed.name());
    replace_arena(&mut ir, "entries", &entries);
    let payload_spans = payload_spans(&ir, removed.name());
    assert!(!payload_spans.is_empty());

    let (report, bytes, round_trip) = write_and_read_back(&ir);
    assert!(!archive_payloads(&bytes).contains_key(removed.name()));
    let omitted = export_messages(&report, "native.records-omitted");
    for id in &payload_spans {
        assert!(
            omitted.iter().all(|message| !message.contains(id.as_str())),
            "{id}: {omitted:?}"
        );
    }
    assert_unheld_entry_reported(&report, round_trip.ir(), &naming, removed.name());
}

/// The ids of the ledger spans that hold the compressed payload of member
/// `name`.
pub(super) fn payload_spans(ir: &cadmpeg_ir::CadIr, name: &str) -> Vec<String> {
    native_arena::<crate::native::ArchiveSpan>(ir, "physical_ledger")
        .into_iter()
        .filter(|span| {
            matches!(&span.role, crate::native::ArchiveSpanRole::CompressedPayload(member)
                if member == name)
        })
        .map(|span| span.id)
        .collect()
}

/// The properties of `ir` that name the archive entry `name`.
pub(super) fn properties_naming(
    ir: &cadmpeg_ir::CadIr,
    name: &str,
) -> Vec<crate::native::PropertyRecord> {
    native_arena::<crate::native::PropertyRecord>(ir, "properties")
        .into_iter()
        .filter(|property| property.side_entries().iter().any(|entry| entry == name))
        .collect()
}

/// Each property in `naming` reads back naming no archive entry `name`, its
/// object is marked for recompute, and the report names it.
pub(super) fn assert_unheld_entry_reported(
    report: &cadmpeg_ir::report::export::ExportReport,
    round_trip: &cadmpeg_ir::CadIr,
    naming: &[crate::native::PropertyRecord],
    name: &str,
) {
    assert!(!naming.is_empty());
    let properties = native_arena::<crate::native::PropertyRecord>(round_trip, "properties");
    let objects = native_arena::<crate::native::ObjectRecord>(round_trip, "objects");
    let omitted = export_messages(report, "native.records-omitted");
    for expected in naming {
        let written = properties
            .iter()
            .find(|property| property.id == expected.id)
            .expect("property written");
        assert!(
            written.side_entries().iter().all(|entry| entry != name),
            "{}",
            expected.id
        );
        assert!(
            omitted
                .iter()
                .any(|message| message.contains(expected.id.as_str())),
            "{}: {omitted:?}",
            expected.id
        );
        let owner = objects
            .iter()
            .find(|object| *object.id() == expected.owner)
            .expect("owner written");
        assert_eq!(
            owner.attributes.get("Touched").map(String::as_str),
            Some("1"),
            "{}",
            expected.owner
        );
    }
}

/// Names every archive entry with its payload.
pub(super) fn archive_payloads(archive: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive.to_vec())).expect("readable ZIP");
    (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).expect("archive entry");
            let name = entry.name().expect("entry name").into_owned();
            let mut payload = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut payload).expect("inflate entry");
            (name, payload)
        })
        .collect()
}

#[test]
fn x65_backslash_entry_is_refused_by_constructor_reader_and_native_record() {
    let error = crate::native::EntryRecord::new(
        "test:native:entry#backslash".into(),
        r"..\outside".into(),
        cadmpeg_core::container::ContainerRole::Auxiliary,
        Vec::new(),
        Vec::new(),
    )
    .expect_err("constructor refuses backslash");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
    let wire = serde_json::json!({"id": "test:native:entry#backslash", "name": r"..\outside", "role": "auxiliary", "byte_len": 0, "sha256": cadmpeg_ir::hash::sha256_hex(&[]), "referenced_by": [], "data": ""});
    let error = cadmpeg_core::serde_erase::json::from_value::<crate::native::EntryRecord>(wire)
        .expect_err("native entry refuses backslash");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let bytes = archive_entries(&[("Document.xml", xml), (r"..\outside", b"payload")]);
    let error = FcstdCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect_err("reader refuses backslash");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
}

#[test]
fn property_edits_use_value_order_when_raw_xml_is_identical() {
    let raw_value = r#"<String value="same"/>"#;
    let mut values = (0..2)
        .map(|order| ValueRecord {
            tag: "String".into(),
            order,
            attributes: [("value".into(), "same".into())].into(),
            text: None,
            raw_xml: raw_value.into(),
        })
        .collect::<Vec<_>>();
    values[1]
        .attributes
        .insert("value".into(), "changed".into());
    let property = PropertyRecord {
        id: "test:property#values".into(),
        owner: "test:object#owner".into(),
        name: "Values".into(),
        type_name: "App::PropertyStringList".into(),
        family: crate::native::PropertyFamily::List,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values,
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(format!(
            r#"<Property name="Values" type="App::PropertyStringList">{raw_value}{raw_value}</Property>"#
        ), 0).unwrap(),
    };
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let output = serialize_property(&ctx, &property).expect("required invariant");
    assert_eq!(output.matches(r#"value="same""#).count(), 1);
    assert_eq!(output.matches(r#"value="changed""#).count(), 1);
    assert!(
        output.find("same").expect("required invariant")
            < output.find("changed").expect("required invariant")
    );
}

#[test]
fn xml_serialization_preserves_normalized_whitespace() {
    let value = ValueRecord {
        tag: "String".into(),
        order: 0,
        attributes: [("value".into(), "a\tb\nc\rd".into())].into(),
        text: Some("a\tb\nc\rd".into()),
        raw_xml: r#"<String value="old">old</String>"#.into(),
    };
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let serialized = serialize_value(&ctx, &value).expect("required invariant");
    assert!(serialized.contains("a&#9;b&#10;c&#13;d"));
    assert_eq!(serialized.matches("&#9;").count(), 2);
    assert_eq!(serialized.matches("&#10;").count(), 2);
    assert_eq!(serialized.matches("&#13;").count(), 2);
}

fn decoded_product() -> cadmpeg_ir::DecodeResult {
    FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source")
}

/// The decoded product with the retained XML of its document `Label` value
/// replaced by `raw_xml`.
fn with_label_value(raw_xml: String) -> cadmpeg_ir::CadIr {
    let mut edited = decoded_product().ir().clone();
    let mut properties = native_arena::<PropertyRecord>(&edited, "properties");
    let label = properties
        .iter_mut()
        .find(|property| {
            property.owner == crate::native::native_id("document", "0") && property.name == "Label"
        })
        .expect("document Label");
    let crate::native::PropertyBody::Persisted { values, .. } = &mut label.body else {
        panic!("the document Label persists its value");
    };
    values[0].raw_xml = raw_xml;
    replace_arena(&mut edited, "properties", &properties);
    edited
}

/// The resource refusal of a write of `ir` under `limits`.
fn write_refusal(
    ir: &cadmpeg_ir::CadIr,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> cadmpeg_core::decode::ResourceLimit {
    let refused = FcstdCodec
        .plan(EncodeInput::new(ir, None).with_limits(limits))
        .and_then(|plan| plan.write_to(&mut Vec::new()));
    let Err(cadmpeg_ir::codec::write::EncodeError::ResourceLimit(limit)) = refused else {
        panic!("a ceiling refuses the write: {refused:?}");
    };
    assert_eq!(limit.scope, cadmpeg_core::decode::LimitScope::Session);
    limit
}

/// The retained property XML parses in the encode session, so the caller's
/// depth ceiling refuses a value nested past it. Under the default ceiling the
/// same document writes.
#[test]
fn the_caller_depth_limit_governs_retained_value_xml() {
    const CEILING: u64 = 64;
    let nesting = usize::try_from(2 * CEILING).expect("nesting fits usize");
    let edited = with_label_value(format!(
        r#"<String value="nested">{}{}</String>"#,
        "<Level>".repeat(nesting),
        "</Level>".repeat(nesting)
    ));

    let mut bytes = Vec::new();
    FcstdCodec
        .plan(EncodeInput::new(&edited, None))
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the default ceiling admits the nested value");

    let mut limits = cadmpeg_core::decode::ResourceLimits::desktop();
    limits.max_recursion_depth = CEILING;
    let limit = write_refusal(&edited, limits);
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RecursionDepth
    );
    assert_eq!(limit.limit, CEILING);
    assert_eq!(limit.operation, "FreeCAD retained property value XML");
}

/// Each attribute of retained value XML is an entity that the write's
/// session admits. An entity ceiling one below the session's count at the
/// wide value refuses the write, and the desktop ceiling writes it.
#[test]
fn retained_value_attributes_count_against_the_entity_ceiling() {
    use cadmpeg_core::decode::test_support::at_charge;
    use cadmpeg_core::decode::{ResourceDimension, ResourceLimits};
    use std::fmt::Write as _;

    let attributes = (0..257).fold(String::new(), |mut tag, index| {
        write!(tag, r#" a{index}="""#).expect("write attribute");
        tag
    });
    let edited = with_label_value(format!(r#"<String value="wide"{attributes}/>"#));
    let values: usize = native_arena::<PropertyRecord>(&edited, "properties")
        .iter()
        .map(|property| {
            if let crate::native::PropertyBody::Persisted { values, .. } = &property.body {
                values.len()
            } else {
                0
            }
        })
        .sum();
    let mut probe = ResourceLimits::desktop();
    probe.max_entities = u64::MAX;
    // Each retained value parse is one charge; the wide one holds `value`
    // and the 257 added attributes.
    let found = (0..values)
        .map(|skip| {
            at_charge(
                ResourceDimension::Entities,
                "FreeCAD retained property value XML",
                skip,
                || write_refusal(&edited, probe),
            )
        })
        .find(|limit| limit.additional == 258)
        .expect("the wide value is charged");
    let mut limits = ResourceLimits::desktop();
    limits.max_entities = found.used + found.additional - 1;
    assert_eq!(write_refusal(&edited, limits), found);
    FcstdCodec
        .plan(EncodeInput::new(&edited, None).with_limits(ResourceLimits::desktop()))
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect("the desktop ceiling writes the wide value");
}

/// Each parse of retained XML charges its text to the input of the write's
/// one session. A ceiling of the stored `Document.xml`'s length admits the
/// declaration read, and the first property parse after it passes the
/// ceiling: the write refuses although no single text exceeds it.
#[test]
fn the_input_ceiling_bounds_every_parse_of_one_write() {
    let decoded = decoded_product();
    let document_xml = native_arena::<crate::native::EntryRecord>(decoded.ir(), "entries")
        .into_iter()
        .find(|entry| entry.name() == "Document.xml")
        .expect("the retained Document.xml")
        .data()
        .len();
    let document_xml = u64::try_from(document_xml).expect("length fits u64");
    let mut limits = cadmpeg_core::decode::ResourceLimits::desktop();
    limits.max_input_bytes = document_xml;
    let limit = write_refusal(decoded.ir(), limits);
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::InputBytes
    );
    assert_eq!((limit.limit, limit.used), (document_xml, document_xml));
    assert_eq!(limit.operation, "FreeCAD retained property XML");
}

#[test]
fn writes_typed_property_edits_and_preserves_other_entries() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let source_entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let mut edited = decoded.ir().clone();
    FcstdCodec
        .set_property_value_attribute(
            &mut edited,
            crate::FcstdPropertyOwner::Document,
            "Label",
            0,
            "value",
            "edited & verified",
        )
        .expect("edit Label");

    let mut encoded = Vec::new();
    let report = FcstdCodec
        .plan(EncodeInput::new(&edited, Some(decoded.source_fidelity())))
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("encode edit");
    assert!(report.losses.is_empty(), "{:#?}", report.losses);
    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode output");
    let output_namespace = round_trip
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace");
    let output_properties = output_namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let output_label = output_properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("document", "0") && property.name == "Label"
        })
        .expect("document Label");
    assert_eq!(
        output_label.values()[0]
            .attributes
            .get("value")
            .map(String::as_str),
        Some("edited & verified")
    );
    let output_entries = output_namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    for source in source_entries
        .iter()
        .filter(|entry| entry.name() != "Document.xml")
    {
        let output = output_entries
            .iter()
            .find(|entry| entry.name() == source.name())
            .expect("preserved entry");
        assert_eq!(output.data(), source.data(), "{}", source.name());
    }
    assert!(crate::test_support::validate_native(round_trip.ir()).is_empty());
}

#[test]
fn mutation_rejects_link_carrier_edits_without_changing_the_graph() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Owner"/><Object type="Part::Feature" name="Target"/></Objects>
<ObjectData Count="2"><Object name="Owner"><Properties Count="1"><Property name="Support" type="App::PropertyLink"><Link value="Target"/></Property></Properties></Object><Object name="Target"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(document)),
            &DecodeOptions::default(),
        )
        .expect("decode link carrier");
    let before = decoded
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties")
        .iter()
        .find(|property| property.name == "Support")
        .expect("Support property")
        .links()
        .to_vec();
    let mut edited = decoded.ir().clone();
    let error = FcstdCodec
        .set_property_value_attribute(
            &mut edited,
            crate::FcstdPropertyOwner::Object("Owner"),
            "Support",
            0,
            "value",
            "Missing",
        )
        .expect_err("link target mutation must refuse stale graph output");
    assert!(error.to_string().contains("graph-aware serializer"));
    let after = edited
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties")
        .iter()
        .find(|property| property.name == "Support")
        .expect("Support property")
        .links()
        .to_vec();
    assert_eq!(after, before);
    assert!(crate::test_support::validate_native(&edited).is_empty());
}

/// A link value edited without its link record has no serializer here: the
/// property stays as the source stores it, and the report names it.
#[test]
fn a_link_value_edit_without_its_link_is_left_as_stored() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Owner"/><Object type="Part::Feature" name="Target"/></Objects>
<ObjectData Count="2"><Object name="Owner"><Properties Count="1"><Property name="Support" type="App::PropertyLink"><Link value="Target"/></Property></Properties></Object><Object name="Target"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(document)),
            &DecodeOptions::default(),
        )
        .expect("decode link carrier");
    let mut edited = decoded.ir().clone();
    let namespace = edited.native.namespace_mut(cadmpeg_ir::codec_id!("fcstd"));
    let mut properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let support = properties
        .iter_mut()
        .find(|property| property.name == "Support")
        .expect("Support property");
    support.values_mut().expect("persisted property")[0]
        .attributes
        .insert("value".into(), "Missing".into());
    let support_id = support.id.clone();
    namespace
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "properties",
            &properties,
        )
        .expect("replace properties");

    let (report, _, round_trip) = write_and_read_back(&edited);
    let support = native_arena::<crate::native::PropertyRecord>(round_trip.ir(), "properties")
        .into_iter()
        .find(|property| property.id == support_id)
        .expect("Support property");
    assert_eq!(
        support.values()[0]
            .attributes
            .get("value")
            .map(String::as_str),
        Some("Target")
    );
    let dropped = export_messages(&report, "edit.dropped");
    assert!(
        dropped
            .iter()
            .any(|message| message.contains(support_id.as_str())),
        "{dropped:?}"
    );
}

#[test]
fn seekable_encoder_matches_the_write_only_fallback() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let mut staged = Vec::new();
    FcstdCodec
        .plan(EncodeInput::new(decoded.ir(), None))
        .and_then(|plan| plan.write_to(&mut staged))
        .expect("write-only fallback");
    let mut streamed = Cursor::new(Vec::new());
    let source_dialect = decoded
        .ir()
        .source
        .as_ref()
        .and_then(cadmpeg_ir::SourceMeta::dialect)
        .expect("decoded FCStd source is classified")
        .dialect();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy {
        limits: cadmpeg_core::decode::ResourceLimits::desktop(),
    };
    let ctx = cadmpeg_core::decode::DecodeContext::new(
        &arena,
        &policy,
        cadmpeg_core::decode::DecodeDepth::Full,
    );
    let resolution = retained_baseline(&ctx, decoded.ir(), source_dialect)
        .expect("the retained Document.xml reads")
        .expect("the decoded schema-4 baseline is preserved");
    crate::writer::write_seekable(&ctx, &mut streamed, &resolution).expect("seekable writer");

    assert_eq!(streamed.into_inner(), staged);
}

/// An object declaration edit has no serializer here, and an entry record
/// whose payload disagrees with its digest does not read: the file is written
/// with the declaration as the source stores it and without the entry, and
/// the report names both.
#[test]
pub(crate) fn writes_around_declaration_edits_and_unreadable_entries() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");

    let mut declaration_edit = decoded.ir().clone();
    let mut objects = native_arena::<crate::native::ObjectRecord>(&declaration_edit, "objects");
    let source_type = objects[0].type_name.clone();
    objects[0].type_name = "App::FeaturePython".into();
    let object_id = objects[0].id().clone();
    replace_arena(&mut declaration_edit, "objects", &objects);
    let (report, _, round_trip) = write_and_read_back(&declaration_edit);
    let written = native_arena::<crate::native::ObjectRecord>(round_trip.ir(), "objects");
    assert_eq!(
        written
            .iter()
            .find(|object| *object.id() == object_id)
            .map(|object| object.type_name.as_str()),
        Some(source_type.as_str())
    );
    let dropped = export_messages(&report, "edit.dropped");
    assert!(
        dropped
            .iter()
            .any(|message| message.contains(object_id.as_str())),
        "{dropped:?}"
    );

    let (mut stale_entry, _, _) = decoded.into_parts();
    let source_entries = native_arena::<crate::native::EntryRecord>(&stale_entry, "entries");
    let mut entries = native_arena::<serde_json::Value>(&stale_entry, "entries");
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
    let naming = properties_naming(&stale_entry, &stale_name);
    replace_arena(&mut stale_entry, "entries", &entries);
    let (report, bytes, round_trip) = write_and_read_back(&stale_entry);
    let payloads = archive_payloads(&bytes);
    assert!(!payloads.contains_key(&stale_name));
    for source in source_entries
        .iter()
        .filter(|entry| entry.name() != "Document.xml" && entry.name() != stale_name)
    {
        assert_eq!(payloads[source.name()], source.data(), "{}", source.name());
    }
    let omitted = export_messages(&report, "native.records-omitted");
    assert!(
        omitted.iter().any(|message| message.contains(&stale_id)),
        "{omitted:?}"
    );
    assert_unheld_entry_reported(&report, round_trip.ir(), &naming, &stale_name);
}

/// A property declaration edit has no serializer here: that property stays
/// as the source stores it, and the value edit beside it is written.
#[test]
fn a_property_declaration_edit_is_left_as_stored_beside_written_edits() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Box" name="Box"/></Objects><ObjectData Count="1"><Object name="Box"><Properties Count="2"><Property name="Length" type="App::PropertyLength"><Float value="10"/></Property><Property name="Label" type="App::PropertyString"><String value="Box"/></Property></Properties></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(xml)),
            &DecodeOptions::default(),
        )
        .expect("Box");
    let mut ir = decoded.ir().clone();
    FcstdCodec
        .set_property_value_attribute(
            &mut ir,
            crate::FcstdPropertyOwner::Object("Box"),
            "Label",
            0,
            "value",
            "Renamed",
        )
        .expect("edit Label");
    let mut properties = native_arena::<crate::native::PropertyRecord>(&ir, "properties");
    let length = properties
        .iter_mut()
        .find(|property| property.name == "Length")
        .expect("Length");
    length.type_name = "App::PropertyDistance".into();
    let length_id = length.id.clone();
    replace_arena(&mut ir, "properties", &properties);

    let (report, _, round_trip) = write_and_read_back(&ir);
    let written = native_arena::<crate::native::PropertyRecord>(round_trip.ir(), "properties");
    let type_and_value = |name: &str| {
        let property = written
            .iter()
            .find(|property| property.name == name)
            .expect("property");
        (
            property.type_name.clone(),
            property.values()[0].attributes.get("value").cloned(),
        )
    };
    assert_eq!(
        type_and_value("Length"),
        ("App::PropertyLength".to_owned(), Some("10".to_owned()))
    );
    assert_eq!(
        type_and_value("Label"),
        ("App::PropertyString".to_owned(), Some("Renamed".to_owned()))
    );
    let dropped = export_messages(&report, "edit.dropped");
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert!(dropped[0].contains(length_id.as_str()), "{dropped:?}");
}

#[test]
fn retaining_writes_preserve_physical_restoration_order_after_cadir_finalization() {
    for schema in ["2", "3", "4"] {
        let (objects_tag, data_tag) = if schema == "2" {
            ("Features", "FeatureData")
        } else {
            ("Objects", "ObjectData")
        };
        let xml = format!(
            r#"<Document SchemaVersion="{schema}" FileVersion="1"><Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="original"/></Property></Properties><{objects_tag} Count="0"/><{data_tag} Count="0"/></Document>"#
        );
        let source = archive_entries(&[
            ("Document.xml", xml.as_bytes()),
            ("ZFirst.brp", b"first"),
            ("ASecond.brp", b"second"),
            ("GuiDocument.xml", br#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera settings=""/></Document>"#),
            ("ZGui.bin", b"gui"),
            ("AOpaque.bin", b"opaque"),
        ]);
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(source), &DecodeOptions::default())
            .expect("required invariant");
        for edit in [false, true] {
            let mut ir = decoded.ir().clone();
            ir.finalize();
            if edit {
                FcstdCodec
                    .set_property_value_attribute(
                        &mut ir,
                        crate::FcstdPropertyOwner::Document,
                        "Label",
                        0,
                        "value",
                        "edited",
                    )
                    .expect("required invariant");
            }
            let mut output = Vec::new();
            FcstdCodec
                .plan(EncodeInput::new(&ir, None))
                .and_then(|plan| plan.write_to(&mut output))
                .expect("required invariant");
            let mut archive =
                zip::ZipArchive::new(Cursor::new(output)).expect("required invariant");
            let names = (0..archive.len())
                .map(|index| {
                    archive
                        .by_index(index)
                        .expect("required invariant")
                        .name()
                        .expect("required invariant")
                        .into_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                names,
                [
                    "Document.xml",
                    "ZFirst.brp",
                    "ASecond.brp",
                    "GuiDocument.xml",
                    "ZGui.bin",
                    "AOpaque.bin"
                ],
                "schema={schema}, edit={edit}"
            );
        }
    }
}

#[test]
fn typed_input_edits_persist_owner_recompute_state_without_touching_outputs_or_noops() {
    for (status, edited, touched) in [
        (0_u64, "15", true),
        (0, "10", false),
        (1 << 7, "15", false),
        (1 << 11, "15", true),
        (1 << 23, "15", false),
        (1 << 27, "15", false),
    ] {
        for declaration in [
            r#"<Object type="Part::Box" name="Box"/>"#,
            r#"<Object type="Part::Box" name="Box" Touched="0"/>"#,
        ] {
            let xml = format!(
                r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1">{declaration}</Objects><ObjectData Count="1"><Object name="Box"><Properties Count="1"><Property name="Length" type="App::PropertyLength" status="{status}"><Float value="10"/></Property></Properties></Object></ObjectData></Document>"#
            );
            let decoded = FcstdCodec
                .decode(
                    &mut Cursor::new(crate::test_support::test_archive::archive(&xml)),
                    &DecodeOptions::default(),
                )
                .expect("decode Box");
            let mut ir = decoded.ir().clone();
            FcstdCodec
                .set_property_value_attribute(
                    &mut ir,
                    crate::FcstdPropertyOwner::Object("Box"),
                    "Length",
                    0,
                    "value",
                    edited,
                )
                .expect("edit Length");
            let objects = ir
                .native
                .namespace("fcstd")
                .expect("namespace")
                .arena_as::<crate::native::ObjectRecord>("objects")
                .expect("objects");
            assert_eq!(
                objects[0].attributes.get("Touched").map(String::as_str) == Some("1"),
                touched
            );
            let mut bytes = Vec::new();
            FcstdCodec
                .plan(EncodeInput::new(&ir, None))
                .and_then(|plan| plan.write_to(&mut bytes))
                .expect("write edit");
            let output = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("decode edit");
            let objects = output
                .ir()
                .native
                .namespace("fcstd")
                .expect("namespace")
                .arena_as::<crate::native::ObjectRecord>("objects")
                .expect("objects");
            assert_eq!(
                objects[0].attributes.get("Touched").map(String::as_str) == Some("1"),
                touched
            );
        }
    }
}

/// Schema 2 declares no recompute mark. An input edit is written, the object
/// keeps its stored shape, and the report names it stale; a label edit needs
/// no recompute.
#[test]
fn schema_two_input_edits_are_written_and_reported_stale() {
    let xml = r#"<Document SchemaVersion="2" FileVersion="1"><Features Count="1"><Feature type="Part::Box" name="Box"/></Features><FeatureData Count="1"><Feature name="Box"><Properties Count="2"><Property name="Length" type="App::PropertyLength"><Float value="10"/></Property><Property name="Label" type="App::PropertyString"><String value="Box"/></Property></Properties></Feature></FeatureData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(xml)),
            &DecodeOptions::default(),
        )
        .expect("schema two Box");
    for (property, value, stale) in [("Length", "15", true), ("Label", "Renamed", false)] {
        let mut ir = decoded.ir().clone();
        FcstdCodec
            .set_property_value_attribute(
                &mut ir,
                crate::FcstdPropertyOwner::Object("Box"),
                property,
                0,
                "value",
                value,
            )
            .expect("edit property");
        let (report, bytes, round_trip) = write_and_read_back(&ir);
        let written = native_arena::<crate::native::PropertyRecord>(round_trip.ir(), "properties")
            .into_iter()
            .find(|candidate| candidate.name == property)
            .expect("property");
        assert_eq!(
            written.values()[0]
                .attributes
                .get("value")
                .map(String::as_str),
            Some(value)
        );
        let document = String::from_utf8(archive_payloads(&bytes)["Document.xml"].clone())
            .expect("UTF-8 Document.xml");
        assert!(!document.contains("Touched"), "{document}");
        let box_id = crate::native::native_id("object", "Box");
        let named_stale = export_messages(&report, "evaluation.stale")
            .iter()
            .any(|message| message.contains(box_id.as_str()));
        assert_eq!(named_stale, stale, "{property}: {:#?}", report.losses);
    }
}

#[test]
fn checked_native_input_record_edits_also_schedule_owner_recomputation() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Box" name="Box"/></Objects><ObjectData Count="1"><Object name="Box"><Properties Count="1"><Property name="Length" type="App::PropertyLength"><Float value="10"/></Property></Properties></Object></ObjectData></Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(xml)),
            &DecodeOptions::default(),
        )
        .expect("Box");
    let mut ir = decoded.ir().clone();
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("fcstd"));
    let mut properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    properties[0].values_mut().expect("values")[0]
        .attributes
        .insert("value".to_owned(), "15".to_owned());
    namespace
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "properties",
            &properties,
        )
        .expect("replace values");
    let mut bytes = Vec::new();
    FcstdCodec
        .plan(EncodeInput::new(&ir, None))
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("write native edit");
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode edit");
    let objects = output
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    assert_eq!(
        objects[0].attributes.get("Touched").map(String::as_str),
        Some("1")
    );
}
