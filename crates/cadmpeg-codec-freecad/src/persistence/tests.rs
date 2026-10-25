// SPDX-License-Identifier: Apache-2.0
//! Document.xml persistence-graph unit tests.
mod metadata_recovery;

use cadmpeg_test_support::wire;

use crate::test_support::test_archive::{archive, archive_entries};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};

use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

/// Admit `bytes` as Document.xml and recover its persistence graph.
fn parse(
    bytes: &[u8],
    schema_version: &str,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<super::Graph, cadmpeg_core::CodecError> {
    let xml = crate::container::admit_document(ctx, bytes)?;
    super::parse_document(&xml, super::Vocabulary::from_declaration(schema_version)?)
}

fn assert_persistence_failures(
    document: &str,
    expected: &[&str],
    message: &str,
) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::source_fidelity::parts::PartState;
    let bytes = archive(document);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    let fidelity = decoded.source_fidelity();
    let mut levels = Vec::new();
    for (id, part) in fidelity
        .parts()
        .iter()
        .filter(|(_, part)| part.state() == &PartState::GivenUp {})
    {
        levels.push(part.level().as_str());
        let from = usize::try_from(part.offset()).unwrap();
        let to = from + usize::try_from(part.byte_len()).unwrap();
        assert_eq!(
            fidelity.retained_record(id.as_str()).unwrap().data(),
            Some(&document.as_bytes()[from..to])
        );
    }
    levels.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(levels, expected, "{document}");
    let errors = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), expected.len());
    assert!(
        errors.iter().all(|loss| loss.message.contains(message)),
        "{errors:?}"
    );
    let stream = fidelity
        .parts()
        .values()
        .find(|part| part.level().as_str() == "freecad.document_xml")
        .unwrap()
        .stream();
    cadmpeg_test_support::contract::parts::check_one_home(
        cadmpeg_ir::CodecFormat::Fcstd,
        &decoded.ir().native,
        fidelity,
        &decoded.report().losses,
        "file",
        |name, range| {
            let source = if name == "file" {
                bytes.as_slice()
            } else if name == stream {
                document.as_bytes()
            } else {
                return None;
            };
            source.get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
        },
    )
    .unwrap();
    decoded
}

#[test]
fn persistence_extension_identity_joins_order_and_name() {
    let owner = "fcstd:native:object#Body";
    assert_eq!(
        super::extension_id(owner, "Proxy", 2),
        crate::native::native_child_id("extension", owner, "2:Proxy")
    );
}

#[test]
fn persistence_link_diagnostics_become_unreadable_payloads() {
    let cases = [
        ("<Property name=\"L\" type=\"App::PropertyLink\"/>",
            "App::PropertyLink requires one value element", "freecad.property"),
        ("<Property name=\"L\" type=\"App::PropertyLinkList\"><LinkList count=\"bad\"/></Property>",
            "link count differs from framed child count", "freecad.property_value"),
        ("<Property name=\"L\" type=\"App::PropertyLinkSub\"><LinkSub value=\"A\" count=\"1\"><Sub/></LinkSub></Property>",
            "link has no value attribute", "freecad.value_element"),
        ("<Property name=\"L\" type=\"App::PropertyXLink\"><XLink file=\"\" name=\"A\" Object=\"A\"/></Property>",
            "link uses ambiguous Object carrier", "freecad.property_value"),
    ];
    for (property, expected, level) in cases {
        let document = format!(
            "<Document SchemaVersion=\"4\"><Objects Count=\"1\"><Object name=\"A\" type=\"T\"/></Objects><ObjectData Count=\"1\"><Object name=\"A\"><Properties Count=\"1\">{property}</Properties></Object></ObjectData></Document>"
        );
        let graph = crate::test_support::with_service_context(document.as_bytes(), |ctx| {
            parse(document.as_bytes(), "4", ctx)
        })
        .expect("bounded invalid link");
        if level == "freecad.property" {
            assert!(graph.properties.is_empty());
        } else if level == "freecad.value_element" {
            let retained = &graph.properties[0];
            assert!(retained.body.is_some());
            assert_eq!(retained.values().len(), 1);
            assert_eq!(retained.values()[0].attributes["value"], "A");
            assert_eq!(retained.links(), &[None]);
            assert!(serde_json::to_value(retained).unwrap()["values"][1].is_null());
        } else {
            assert!(graph.properties[0].body.is_none());
        }
        let unreadable: Vec<_> = graph
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "record.unreadable")
            .collect();
        assert_eq!(unreadable.len(), 1);
        assert_eq!(unreadable[0].severity, cadmpeg_ir::report::Severity::Error);
        assert!(unreadable[0].message.contains(level));
        assert!(unreadable[0].message.contains(expected));
    }
}

#[test]
fn x63_decode_counts_source_image_in_retained_budget() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object name="A" type="Part::Feature"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let bytes = archive(document);
    FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode the persistence document");

    let limit = cadmpeg_test_support::decode::resource_refusal_at(
        &FcstdCodec,
        &bytes,
        &mut DecodeOptions::default(),
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain FCStd source image",
        0,
    );
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    );
    assert_eq!(limit.operation, "retain FCStd source image");
}

fn parse_document_graph(document: &str) -> Result<super::Graph, cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(document.as_bytes(), &arena, &policy)?;
    let xml = crate::container::admit_document(&ctx, document.as_bytes())?;
    let (_facts, schema_version) = crate::container::parse_document(&xml, &mut Vec::new())
        .map_err(|error| match error {
            cadmpeg_core::CodecError::WrongFormat(message) => {
                cadmpeg_core::CodecError::Malformed(message)
            }
            error => error,
        })?;
    parse(document.as_bytes(), &schema_version, &ctx)
}

#[test]
pub(crate) fn schema_three_uses_the_object_envelope_and_defaults_file_version() {
    let document = r#"<Document SchemaVersion="3">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Legacy"/></Property></Properties>
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Thing"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let bytes = archive(document);
    let summary = FcstdCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("legacy inspection");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=3"));
    assert!(summary.notes.iter().any(|note| note == "FileVersion=0"));
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("schema-three decode");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].type_name, "App::FeaturePython");
    assert_eq!(properties.len(), 2);
    assert_eq!(
        properties[1].links()[0].as_ref().expect("link").object(),
        Some(objects[0].id().as_str())
    );
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
}

#[test]
pub(crate) fn schema_two_uses_the_feature_envelope_and_common_property_grammar() {
    let document = r#"<Document SchemaVersion="2" ProgramVersion="0.13">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Document"/></Property></Properties>
<Features Count="2"><Feature type="App::Feature" name="First"/><Feature type="App::FeaturePython" name="Second"/></Features>
<FeatureData Count="2"><Feature name="First"><Properties Count="0"/></Feature><Feature name="Second"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="First"/></Property></Properties></Feature></FeatureData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("schema-two decode");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    assert_eq!(
        objects
            .iter()
            .map(|object| object.name().as_str())
            .collect::<Vec<_>>(),
        ["First", "Second"]
    );
    assert_eq!(properties.len(), 2);
    assert_eq!(
        properties[1].links()[0].as_ref().expect("link").object(),
        Some(objects[0].id().as_str())
    );
    assert!(objects.iter().all(|object| object.persistent_id.is_none()));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_duplicate_root_property_containers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Properties Count="1"><Property name="First" type="App::PropertyString"><String value="one"/></Property></Properties>
<Properties Count="1"><Property name="Second" type="App::PropertyString"><String value="two"/></Property></Properties>
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let bytes = archive(document);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("only the duplicate root Properties elements are given up");
    assert_eq!(decoded.ir().scope, cadmpeg_ir::ContentScope::full());
    assert_eq!(decoded.report().scope(), cadmpeg_ir::ContentScope::full());
    assert_eq!(decoded.ir().model.entity_count(), 0);
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    let facts = native
        .arena_as::<crate::native::DocumentFacts>("document")
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].id, crate::native::native_id("document", "0"));
    assert_eq!(facts[0].object_count, 0);
    assert!(native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty());
    let entries = native
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let [entry] = entries.as_slice() else {
        panic!("one independently admitted archive entry: {entries:?}")
    };
    assert_eq!(entry.name(), "Document.xml");
    assert_eq!(
        entry.id(),
        crate::native::native_id("entry", "Document.xml")
    );
    assert!(entry.referenced_by().is_empty());
    let fidelity = decoded.source_fidelity();
    let xml = roxmltree::Document::parse(document).unwrap();
    assert_eq!(
        fidelity.unrepresented(),
        cadmpeg_ir::ArenaClasses::from([
            cadmpeg_ir::ArenaClass::Model,
            cadmpeg_ir::ArenaClass::Design,
        ])
    );
    let expected = xml
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Properties"))
        .map(|node| node.range())
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), 2);
    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    assert_eq!(unknowns.len(), 2);
    let failures = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 2);
    for range in &expected {
        let offset = u64::try_from(range.start).unwrap();
        let length = u64::try_from(range.len()).unwrap();
        let unknown = unknowns
            .iter()
            .find(|unknown| fidelity.parts()[&unknown.id].offset() == offset)
            .unwrap();
        assert_eq!(unknown.links.len(), 1);
        assert_eq!(unknown.links[0].as_str(), facts[0].id);
        let part = &fidelity.parts()[&unknown.id];
        assert_eq!(part.level().as_str(), "freecad.properties");
        assert_eq!(
            part.state(),
            &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
        );
        assert_eq!(part.byte_len(), length);
        let parent = &fidelity.parts()[part.parent().unwrap()];
        assert_eq!(parent.level().as_str(), "freecad.document");
        assert_eq!(
            parent.state(),
            &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
        );
        let xml_part = &fidelity.parts()[parent.parent().unwrap()];
        assert_eq!(xml_part.level().as_str(), "freecad.document_xml");
        let member = &fidelity.parts()[xml_part.parent().unwrap()];
        assert_eq!(member.level().as_str(), "zip.entry");
        let archive = &fidelity.parts()[member.parent().unwrap()];
        assert_eq!(archive.level().as_str(), "zip.archive");
        assert!(archive.parent().is_none());
        for ancestor in [xml_part, member, archive] {
            assert_eq!(
                ancestor.state(),
                &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
            );
        }
        let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
        assert_eq!(retained.data(), Some(&document.as_bytes()[range.clone()]));
        assert_eq!(retained.offset(), offset);
        assert_eq!(retained.byte_len(), length);
        assert_eq!(retained.stream(), part.stream());
        assert_eq!(
            retained.sha256(),
            cadmpeg_ir::hash::sha256_hex(&document.as_bytes()[range.clone()])
        );
        assert!(fidelity.record_layout()[unknown.id.as_str()]
            .reads()
            .is_empty());
        for layout in fidelity.record_layout().values() {
            if let Some(reads) = layout.reads().get(part.stream()) {
                assert!(reads
                    .iter()
                    .all(|read| read.end <= offset || read.start >= offset + length));
            }
        }
        let failure = failures
            .iter()
            .find(|loss| {
                loss.provenance
                    .as_ref()
                    .is_some_and(|location| location.offset == offset)
            })
            .unwrap();
        assert_eq!(failure.severity, cadmpeg_ir::report::Severity::Error);
        assert!(failure
            .message
            .contains("Document.xml has multiple root Properties containers"));
        let location = failure.provenance.as_ref().unwrap();
        assert_eq!(location.stream_name(), Some(part.stream()));
        assert_eq!(location.byte_len(), Some(length));
    }
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(
                |part| part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
            .count(),
        2
    );
    assert!(!fidelity.parts().values().any(|part| matches!(
        part.level().as_str(),
        "freecad.property" | "freecad.property_value"
    )));
    assert!(fidelity
        .record_layout()
        .contains_key(&crate::native::native_id("document", "0")));
    assert!(fidelity.record_layout().contains_key(entry.id()));
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn rejects_duplicate_property_names_for_one_owner() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Part" name="Part"/></Objects>
<ObjectData Count="1"><Object name="Part"><Properties Count="2">
 <Property name="Label" type="App::PropertyString"><String value="one"/></Property>
 <Property name="Label" type="App::PropertyString"><String value="two"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let bytes = archive(document);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("each duplicate property has its own bounded recovery home");
    let native = decoded.ir().native.namespace("fcstd").unwrap();
    let objects = native
        .arena_as::<crate::native::ObjectRecord>("objects")
        .unwrap();
    let [object] = objects.as_slice() else {
        panic!("the independently framed Part declaration survives")
    };
    assert_eq!(object.name(), "Part");
    assert_eq!(object.type_name, "App::Part");
    assert!(native
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap()
        .is_empty());
    assert!(!decoded
        .ir()
        .model
        .attributes
        .iter()
        .any(|attribute| attribute.name == "Label"));

    let unknowns = decoded.ir().native_unknowns("fcstd").unwrap();
    assert_eq!(unknowns.len(), 2);
    let fidelity = decoded.source_fidelity();
    let failures = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 2);
    for value in ["one", "two"] {
        let source = format!("<Property name=\"Label\" type=\"App::PropertyString\"><String value=\"{value}\"/></Property>");
        let start = document.find(&source).unwrap();
        let end = start + source.len();
        let unknown = unknowns
            .iter()
            .find(|unknown| fidelity.parts()[&unknown.id].offset() == u64::try_from(start).unwrap())
            .unwrap();
        let part = &fidelity.parts()[&unknown.id];
        assert_eq!(part.level().as_str(), "freecad.property");
        assert_eq!(
            part.state(),
            &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
        );
        assert_eq!(part.offset(), u64::try_from(start).unwrap());
        assert_eq!(part.byte_len(), u64::try_from(end - start).unwrap());
        let parent = &fidelity.parts()[part.parent().unwrap()];
        assert_eq!(parent.level().as_str(), "freecad.properties");
        assert_eq!(
            parent.state(),
            &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
        );
        let mut ancestor = parent.parent();
        while let Some(id) = ancestor {
            let parent = &fidelity.parts()[id];
            assert_eq!(
                parent.state(),
                &cadmpeg_ir::source_fidelity::parts::PartState::Decoded {}
            );
            ancestor = parent.parent();
        }
        let retained = fidelity.retained_record(unknown.id.as_str()).unwrap();
        assert_eq!(retained.data(), Some(&document.as_bytes()[start..end]));
        assert_eq!(retained.stream(), part.stream());
        assert_eq!(retained.offset(), part.offset());
        assert_eq!(retained.byte_len(), part.byte_len());
        assert_eq!(
            retained.sha256(),
            cadmpeg_ir::hash::sha256_hex(&document.as_bytes()[start..end])
        );
        assert!(fidelity.record_layout()[unknown.id.as_str()]
            .reads()
            .is_empty());
        for layout in fidelity.record_layout().values() {
            if let Some(reads) = layout.reads().get(part.stream()) {
                assert!(reads.iter().all(|range| range.end <= part.offset()
                    || range.start >= part.offset() + part.byte_len()));
            }
        }
        let failure = failures
            .iter()
            .find(|loss| loss.provenance.as_ref().unwrap().offset == part.offset())
            .unwrap();
        assert_eq!(failure.severity, cadmpeg_ir::report::Severity::Error);
        assert!(failure
            .message
            .contains("duplicate property name Label for object fcstd:native:object#Part"));
        let location = failure.provenance.as_ref().unwrap();
        assert_eq!(location.stream_name(), Some(part.stream()));
        assert_eq!(location.offset, part.offset());
        assert_eq!(location.byte_len(), Some(part.byte_len()));
    }
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(
                |part| part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
            )
            .count(),
        2
    );
    assert!(!fidelity
        .parts()
        .values()
        .any(|part| part.level().as_str() == "freecad.property_value"));
    assert!(fidelity.record_layout().contains_key(object.id()));
    assert_eq!(crate::source_archive::image(fidelity).unwrap(), bytes);
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .unwrap()
        .is_ok());
}

#[test]
fn retains_nested_xlink_value_children_as_unreadable_source() {
    let documents = [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1">
<Property name="Source" type="App::PropertyXLink"><XLink file="" name="Target"><XLink file="" name="Nested"/></XLink></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1">
<Property name="Source" type="App::PropertyXLink"><XLink file="" name="Target" sub="Face1"><Sub value="Face2"/></XLink></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1">
<Property name="Source" type="App::PropertyXLink"><XLink file="" name="Target" count="1"><Sub value="Face1"><Sub value="Face2"/></Sub></XLink></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::FeaturePython" name="Thing"/></Objects>
<ObjectData Count="1"><Object name="Thing"><Properties Count="1">
<Property name="Source" type="App::PropertyXLink"><XLink file="" name="Target" count="0"/></Property>
</Properties></Object></ObjectData></Document>"#,
    ];
    for document in documents {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect("bounded bad link retained");
        let properties = decoded
            .ir()
            .native
            .namespace("fcstd")
            .expect("namespace")
            .arena_as::<crate::native::PropertyRecord>("properties")
            .expect("properties");
        assert!(matches!(properties[0].body, None));
        assert!(decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.unreadable"
                && loss.severity == cadmpeg_ir::report::Severity::Error));
        assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    }
}

#[test]
pub(crate) fn legacy_schema_dispatch_rejects_wrong_envelopes_and_unmatched_objectdata() {
    let cases = [
        r#"<Document SchemaVersion="2"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="3"><Features Count="0"/><FeatureData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="2"><Features Count="1"><Feature type="App::Feature" name="A"/></Features><FeatureData Count="1"><Feature name="B"/></FeatureData></Document>"#,
    ];
    for (case, document) in cases.into_iter().enumerate() {
        let (levels, message): (&[&str], &str) = if case < 2 {
            (
                &["freecad.objects", "freecad.object_data_section"],
                "wrong schema envelope",
            )
        } else {
            (&["freecad.object_data"], "has no readable declaration")
        };
        let decoded = assert_persistence_failures(document, levels, message);
        let objects = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::ObjectRecord>("objects")
            .unwrap();
        assert_eq!(objects.len(), usize::from(case == 2));
        if case == 2 {
            assert_eq!(objects[0].name(), "A");
        }
    }
}

#[test]
fn recovers_objects_dynamic_properties_links_and_side_entries() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Demo"/></Property></Properties>
<Objects Count="2" Dependencies="1">
<ObjectDeps Name="Body" Count="1" AllowPartial="2"><Dep Name="Sketch"/></ObjectDeps>
<ObjectDeps Name="Sketch" Count="0"/>
<Object type="PartDesign::Body" name="Body" id="1" Touched="1"/>
<Object type="PartDesign::Feature" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
<Object name="Body" Extensions="True"><Extensions Count="1"><Extension type="Demo::Extension" name="Demo"><Properties Count="1"><Property name="ExtensionValue" type="App::PropertyString"><String value="kept"/></Property></Properties></Extension></Extensions><Properties Count="4" TransientCount="1">
<_Property name="TransientState" type="App::PropertyInteger" status="8"/>
<Property name="Support" type="App::PropertyLinkSub" status="4" group="Attachment" doc="Support object" attr="2" ro="1" hide="0"><LinkSub value="Sketch" count="1"><Sub value="Face1"/></LinkSub></Property>
<Property name="Members" type="App::PropertyLinkList"><LinkList count="2"><Link value="Sketch"/><Link value=""/></LinkList></Property>
<Property name="Payload" type="App::PropertyFileIncluded"><File file="Payload.bin"/></Property>
<Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="" file="Shape.brp"/></Property>
</Properties></Object>
<Object name="Sketch"><Properties Count="0"></Properties></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Payload.bin", b"payload"),
        (
            "Shape.brp",
            b"\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 4\n1 10 20 30 1 0 0\n7 0 0 2 3 2 0 0 0 5 0 0 10 0 0 0 3 1 3\n8 0 5 1 0 0 0 1 0 0\n9 2 0 0 1 1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 5\n1 0 0 0 0 0 1 1 0 0 0 1 0\n9 0 0 0 0 1 1 2 2 2 2 0 0 0 0 1 0 1 0 0 1 1 0 0 2 1 2 0 2 1 2\n6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 1\n3 1 0 0.01 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 0\n*",
        ),
    ]);
    let mut original = zip::ZipArchive::new(Cursor::new(&bytes)).expect("fixture archive");
    let mut expanded_lengths = std::collections::BTreeMap::new();
    for index in 0..original.len() {
        let entry = original.by_index(index).expect("fixture member");
        expanded_lengths.insert(
            entry.name().expect("fixture member name").into_owned(),
            entry.size(),
        );
    }
    let result = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("decode graph");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let extensions = namespace
        .arena_as::<crate::native::ExtensionRecord>("extensions")
        .expect("extensions");
    assert_eq!(objects.len(), 2);
    assert_eq!(
        objects[0]
            .dependency_allow_partial
            .map(std::num::NonZeroU64::get),
        Some(2)
    );
    assert_eq!(objects[1].dependency_allow_partial, None);
    assert_eq!(extensions.len(), 1);
    assert_eq!(extensions[0].owner, "fcstd:native:object#Body");
    let extension_value = properties
        .iter()
        .find(|property| property.name == "ExtensionValue")
        .expect("extension property");
    assert_eq!(extension_value.owner, extensions[0].id);
    assert_eq!(objects[0].dependencies, vec!["fcstd:native:object#Sketch"]);
    let support = properties
        .iter()
        .find(|property| property.name == "Support")
        .expect("support");
    assert_eq!(support.owner, "fcstd:native:object#Body");
    assert_eq!(
        support.links()[0].as_ref().expect("link").object(),
        Some("fcstd:native:object#Sketch")
    );
    assert_eq!(support.family, crate::native::PropertyFamily::Link);
    assert_eq!(
        support.links()[0].as_ref().expect("link").subelements(),
        vec!["Face1"]
    );
    assert!(matches!(
        &support.body,
        Some(crate::native::PropertyBody::Persisted { .. })
    ));
    let dynamic = &support.dynamic;
    let Some(_) = &support.body else {
        panic!("support property is not persisted");
    };
    assert_eq!(dynamic.as_ref().and_then(|meta| meta.read_only), Some(true));
    let members = properties
        .iter()
        .find(|property| property.name == "Members")
        .expect("members");
    assert_eq!(members.links().len(), 2);
    assert_eq!(
        members.links()[0].as_ref().expect("link").object(),
        Some("fcstd:native:object#Sketch")
    );
    assert!(members.links()[1].is_none());
    let transient = properties
        .iter()
        .find(|property| property.name == "TransientState")
        .expect("transient");
    assert!(transient.is_transient());
    assert_eq!(transient.status, Some(8));
    let payload = properties
        .iter()
        .find(|property| property.name == "Payload")
        .expect("payload");
    // The original File spelling is not this registered family's FileIncluded
    // carrier; the independent property declaration and archive member remain.
    assert!(payload.body.is_none());
    assert!(payload.side_entries().is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code.local_code() == "record.unreadable"
            && loss.message.contains("freecad.property_value")
            && loss.message.contains(&payload.id)
    }));
    let shape = properties
        .iter()
        .find(|property| property.name == "Shape")
        .expect("shape");
    assert_eq!(shape.family, crate::native::PropertyFamily::Geometry);
    assert_eq!(shape.side_entries(), vec!["Shape.brp"]);
    let shape_payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .expect("shape payloads");
    assert_eq!(shape_payloads.len(), 1);
    assert_eq!(shape_payloads[0].payload.topology_version(), Some(1));
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.curves.len(), 8);
    match &result.ir().model.curves[0].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            assert_eq!([origin.x, origin.y, origin.z], [10.0, 20.0, 30.0]);
            assert_eq!([direction.x, direction.y, direction.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected curve {other:?}"),
    }
    match &result.ir().model.curves[1].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            assert_eq!(nurbs.degree(), 2);
            assert_eq!(nurbs.control_points().len(), 3);
            assert_eq!(nurbs.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            assert!(nurbs.weights().is_none());
        }
        other => panic!("unexpected curve {other:?}"),
    }
    assert_eq!(result.ir().model.procedural_curves.len(), 2);
    match result.ir().model.procedural_curves[0].definition() {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(definition_payload) => {
            let parameter_range = definition_payload.parameter_range();
            assert_eq!(parameter_range.endpoints(), [0.0, 5.0]);
        }
        other => panic!("unexpected trimmed construction {other:?}"),
    }
    match result.ir().model.procedural_curves[1].definition() {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset(payload) => {
            let distance = wire::field::<f64>(&payload, "distance");
            let cadmpeg_ir::geometry::OffsetSide::Direction {
                direction,
                support: None,
            } = payload.side()
            else {
                panic!("unexpected offset construction {payload:?}");
            };
            assert_eq!(distance, 2.0);
            assert_eq!([direction.x, direction.y, direction.z], [0.0, 0.0, 1.0]);
        }
        other => panic!("unexpected offset construction {other:?}"),
    }
    assert_eq!(result.ir().model.surfaces.len(), 7);
    match &result.ir().model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            plane_surface,
        )) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            assert_eq!([origin.x, origin.y, origin.z], [0.0, 0.0, 0.0]);
            assert_eq!([normal.x, normal.y, normal.z], [0.0, 0.0, 1.0]);
            assert_eq!([u_axis.x, u_axis.y, u_axis.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected surface {other:?}"),
    }
    assert_eq!(result.ir().model.procedural_surfaces.len(), 4);
    assert!(matches!(
        result.ir().model.procedural_surfaces[0].definition(),
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion(_)
    ));
    assert!(
        match result.ir().model.procedural_surfaces[1].definition() {
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution(matched_payload) =>
                matches!((&matched_payload.parameter_interval(),), (None,)),
            _ => false,
        }
    );
    assert!(
        match result.ir().model.procedural_surfaces[2].definition() {
            cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset(matched_payload) => matches!(
                (matched_payload.u_sense(), matched_payload.v_sense(),),
                (None, None,)
            ),
            _ => false,
        }
    );
    assert!(matches!(
        result.ir().model.procedural_surfaces[3].definition(),
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset(_)
    ));
    match &result.ir().model.surfaces[1].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
            assert_eq!((nurbs.u_degree(), nurbs.v_degree()), (1, 1));
            assert_eq!((nurbs.u_count(), nurbs.v_count()), (2, 2));
            assert_eq!(nurbs.poles().len(), 4);
            assert_eq!(nurbs.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert!(nurbs.weights().is_none());
        }
        other => panic!("unexpected surface {other:?}"),
    }
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].vertices().len(), 3);
    assert_eq!(result.ir().model.tessellations[0].triangles(), [[0, 1, 2]]);
    assert!(result.ir().model.tessellations[0].body.is_none());
    assert!(result.ir().model.tessellations[0].faces.is_empty());
    assert_eq!(
        result.ir().model.tessellations[0]
            .chordal_deflection()
            .map(cadmpeg_ir::scalar::NonNegativeReal::get),
        Some(0.01)
    );
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let payload_entry = entries
        .iter()
        .find(|entry| entry.name() == "Payload.bin")
        .expect("payload entry");
    assert!(payload_entry.referenced_by().is_empty());
    assert_eq!(
        crate::test_support::source_fidelity::entry_bytes(
            result.source_fidelity(),
            payload_entry.id()
        ),
        b"payload"
    );
    let ledger = result
        .source_fidelity()
        .freecad_ledgers()
        .values()
        .next()
        .expect("FCStd source ledgers")
        .logical_ledger
        .clone();
    for entry in &entries {
        let mut spans = ledger
            .iter()
            .filter(|span| span.entry == entry.name())
            .collect::<Vec<_>>();
        spans.sort_by_key(|span| span.span.start());
        assert_eq!(spans.first().map(|span| span.span.start()), Some(0));
        assert_eq!(
            spans.last().map(|span| span.span.end()),
            Some(expanded_lengths[entry.name()])
        );
        assert!(spans
            .windows(2)
            .all(|pair| pair[0].span.end() == pair[1].span.start()));
    }
    assert!(ledger
        .iter()
        .filter(|span| span.entry == "Shape.brp")
        .all(|span| span.classification.as_str() == "typed"));
    assert!(ledger
        .iter()
        .filter(|span| span.entry == "Payload.bin")
        .all(|span| span.classification.as_str() == "named_opaque"));
    assert!(ledger
        .iter()
        .any(|span| span.entry == "Document.xml" && span.classification.as_str() == "typed"));
    assert!(ledger.iter().any(|span| {
        span.entry == "Document.xml" && span.classification.as_str() == "structural"
    }));
    let coverage = vec![result
        .source_fidelity()
        .freecad_ledgers()
        .values()
        .next()
        .expect("FCStd source ledgers")
        .byte_coverage
        .clone()];
    assert_eq!(coverage.len(), 1);
    assert!(coverage[0].exact);
    assert_eq!(coverage[0].logical_entry_count, entries.len());
    assert_eq!(
        coverage[0].logical_byte_len,
        entries
            .iter()
            .map(|entry| expanded_lengths[entry.name()])
            .sum::<u64>()
    );
    assert_eq!(
        coverage[0].classification_bytes.values().sum::<u64>(),
        coverage[0].logical_byte_len
    );
    assert!(coverage[0]
        .named_opaque_entries
        .contains(&"Payload.bin".to_owned()));
    let findings = crate::test_support::validate_native(result.ir());
    assert!(findings.is_empty(), "{findings:#?}");

    // #209: source accounting belongs to SourceFidelity, never document arenas.
    let native_arenas = result.ir().native.namespace("fcstd").unwrap().arenas();
    for source_only in ["logical_ledger", "physical_ledger", "byte_coverage"] {
        assert!(!native_arenas.contains_key(source_only));
    }
}

#[test]
fn rejects_inconsistent_object_dependency_envelopes() {
    let cases = [
        r#"<Document SchemaVersion="4"><Objects Count="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1" Dependencies="1"><ObjectDeps Name="A" Count="1"><Dep Name="Missing"/></ObjectDeps><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="B" Count="0"/><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
    ];

    for (case, document) in cases.into_iter().enumerate() {
        let (levels, message): (&[&str], &str) = match case {
            0 => (&["freecad.object_deps"], "dependency envelope"),
            1 => (&["freecad.objects"], "dependency population"),
            2 => (&["freecad.object_dep"], "never declared"),
            3 => (&["freecad.object_deps"], "duplicate ObjectDeps name"),
            _ => (
                &["freecad.object_deps", "freecad.object_deps"],
                "dependency envelope",
            ),
        };
        let decoded = assert_persistence_failures(document, levels, message);
        let objects = decoded
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::ObjectRecord>("objects")
            .unwrap();
        assert_eq!(objects.len(), [1, 0, 1, 2, 2][case]);
        assert!(objects.iter().all(|object| object.dependencies.is_empty()));
    }
}

#[test]
fn rejects_ambiguous_persistence_carriers() {
    let cases = [
        r#"<Document schemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4" schemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="0"/><Objects Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="0"/><ObjectData Count="0"/><ObjectData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="2" Dependencies="1"><ObjectDeps Name="A" Count="0"/><Object type="App::Feature" name="A"/><ObjectDeps Name="B" Count="0"/><Object type="App::Feature" name="B"/></Objects><ObjectData Count="2"><Object name="A"/><Object name="B"/></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="2"><Property name="Same" type="App::PropertyString"/><Property name="Same" type="App::PropertyString"/></Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="0"/><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="0"/><Extensions Count="0"/><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2"><Extension type="Vendor::First" name="Same"/><Extension type="Vendor::Second" name="Same"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2"><Extension type="Vendor::Same" name="First"/><Extension type="Vendor::Same" name="Second"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A" Extensions="True"><Properties Count="0"/><Extensions Count="0"/></Object></ObjectData></Document>"#,
    ];

    for (case, document) in cases.into_iter().enumerate() {
        let (levels, message): (&[&str], &str) = match case {
            0 | 1 => (&["freecad.document"], "unsupported schemaVersion"),
            2 => (
                &["freecad.objects"],
                "duplicate object declarations section",
            ),
            3 => (
                &["freecad.object_data_section"],
                "duplicate object data section",
            ),
            4 => (&["freecad.object_deps"], "dependency envelope"),
            5 => (
                &["freecad.property", "freecad.property"],
                "duplicate property name Same",
            ),
            6 => (&["freecad.properties"], "duplicate direct Properties"),
            8 | 9 => (&["freecad.extension"], "duplicate extension name or type"),
            _ => (
                &["freecad.extensions"],
                "duplicate or misordered Extensions",
            ),
        };
        let decoded = assert_persistence_failures(document, levels, message);
        if case >= 4 {
            let objects = decoded
                .ir()
                .native
                .namespace("fcstd")
                .unwrap()
                .arena_as::<crate::native::ObjectRecord>("objects")
                .unwrap();
            assert_eq!(objects.len(), if case == 4 { 2 } else { 1 });
            assert!(objects.iter().any(|object| object.name() == "A"));
        }
    }
}

#[test]
fn binds_nested_extension_properties_to_their_enclosing_record() {
    let document = r#"<Document SchemaVersion="4">
<Objects Count="1"><Object type="App::Feature" name="A"/></Objects>
<ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="2">
<Extension type="Vendor::First" name="First"><Properties Count="1"><Property name="FirstValue" type="App::PropertyString"><String value="first"/></Property></Properties></Extension>
<Extension type="Vendor::Second" name="Second"><Properties Count="1"><Property name="SecondValue" type="App::PropertyString"><String value="second"/></Property></Properties></Extension>
</Extensions><Properties Count="0"/></Object></ObjectData></Document>"#;
    let graph = parse_document_graph(document).expect("extension graph");
    let first = graph
        .extensions
        .iter()
        .find(|extension| extension.name == "First")
        .expect("first extension");
    let second = graph
        .extensions
        .iter()
        .find(|extension| extension.name == "Second")
        .expect("second extension");
    assert_eq!(
        graph
            .properties
            .iter()
            .find(|property| property.name == "FirstValue")
            .expect("first property")
            .owner,
        first.id
    );
    assert_eq!(
        graph
            .properties
            .iter()
            .find(|property| property.name == "SecondValue")
            .expect("second property")
            .owner,
        second.id
    );
}

#[test]
fn native_validation_rejects_duplicate_extension_identity() {
    let document = r#"<Document SchemaVersion="4">
<Objects Count="1"><Object type="App::Feature" name="A"/></Objects>
<ObjectData Count="1"><Object name="A" Extensions="True"><Extensions Count="1"><Extension type="Vendor::Extension" name="Extension"/></Extensions><Properties Count="0"/></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("extension graph");
    let mut corrupted = result.ir().clone();
    let mut extensions = corrupted
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ExtensionRecord>("extensions")
        .expect("extensions")
        .clone();
    extensions.push(extensions[0].clone());
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "extensions",
            &extensions,
        )
        .expect("replace extensions");
    let findings = crate::test_support::validate_native(&corrupted);
    assert!(findings.iter().any(|finding| {
        finding.message.contains("duplicate FCStd native identity")
            || finding.message.contains("duplicates extension name")
    }));
}

#[test]
fn unknown_property_runtime_names_do_not_select_a_family_by_substring() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="1"><Property name="Custom" type="Vendor::PropertyLinkAndPropertyString"><Link value="A"/></Property></Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("unknown property runtime type is retained");
    let property = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties")
        .into_iter()
        .find(|property| property.name == "Custom")
        .expect("custom property");
    assert_eq!(property.family, crate::native::PropertyFamily::Unknown);
    assert!(property.links().is_empty());
}

#[test]
fn empty_and_absent_xlink_file_attributes_decode_to_one_typed_value() {
    let link = |markup: &str| {
        let parsed = roxmltree::Document::parse(markup).expect("parse XLink markup");
        super::xlink(parsed.root_element()).expect("decode XLink")
    };
    let empty = link(r#"<XLink file="" name="Body"/>"#);
    let absent = link(r#"<XLink name="Body"/>"#);
    assert_eq!(empty, absent);
    let empty = empty.as_ref().expect("link");
    assert_eq!(empty.document(), None);
    assert_eq!(empty.document_attribute(), None);
}

#[test]
fn both_xlink_list_property_types_use_xlink_sub_list_carriers() {
    for type_name in ["App::PropertyXLinkList", "App::PropertyXLinkSubList"] {
        let markup = format!(
            r#"<Property name="References" type="{type_name}"><XLinkSubList count="2"><XLink name="Local"/><XLink file="parts.FCStd" name="Remote" sub="Face1"/></XLinkSubList></Property>"#
        );
        let xml = roxmltree::Document::parse(&markup).unwrap();
        let links = { super::parse_link_targets(xml.root_element(), type_name).unwrap() };
        assert_eq!(links.len(), 2);
        let first = links[0].as_ref().expect("first link");
        let second = links[1].as_ref().expect("second link");
        assert_eq!(first.object(), Some("Local"));
        assert_eq!(first.document_name(), None);
        assert_eq!(second.object(), Some("Remote"));
        assert_eq!(second.document_name(), Some("parts.FCStd"));
        assert_eq!(second.subelements(), ["Face1"]);
        let invalid = markup.replace("XLinkSubList", "XLinkList");
        let xml = roxmltree::Document::parse(&invalid).unwrap();
        assert!(super::parse_link_targets(xml.root_element(), type_name).is_err());
    }
}

#[test]
fn link_child_framing_is_checked_before_reading_children() {
    for xml in [
        r#"<Links count="0"><Link/><Link/><Link/></Links>"#,
        r#"<Links count="1000000"/>"#,
        r#"<Links count="1"><Sub/></Links>"#,
    ] {
        let tree = roxmltree::Document::parse(xml).expect("framed XML");
        let result = super::counted_children(tree.root_element(), "Link", "App::PropertyLinkList");
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn property_value_xml_above_the_copy_bound_is_unreadable_with_a_loss() {
    let payload = "a".repeat(6 * 1024 * 1024);
    let document = format!(
        r#"<Properties Count="1"><Property name="P" type="App::PropertyString"><Outer><Middle><Leaf>{payload}</Leaf></Middle></Outer></Property></Properties>"#
    );
    let xml = roxmltree::Document::parse(&document).expect("XML");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, view) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        document.as_bytes(),
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("source");
    let source = crate::xml_framing::XmlSource::new(&ctx, view).expect("source elements");
    let entries = std::collections::BTreeMap::new();
    let context = crate::element_map::adopted::Context::new(&entries, Some(1));
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let properties = super::document_properties::decode_owned_scanned(
        &ctx,
        &source,
        0,
        &crate::native::native_id("document", "0"),
        &context,
        &mut output,
    )
    .expect("source-backed values need no overlapping XML copies");
    assert_eq!(properties.properties.len(), 1);
    let property = &properties.properties[0];
    assert!(property.body.is_none());
    assert!(property.values().is_empty());
    assert!(!properties.sources[&property.id].has_values());
    assert_eq!(
        &document[properties.sources[&property.id].record_range()],
        &document[xml
            .root_element()
            .first_element_child()
            .expect("property")
            .range()]
    );
    let failures: Vec<_> = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| part.state() == &cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {})
        .collect();
    let [failure] = failures.as_slice() else {
        panic!("one actual invalid String value: {failures:?}")
    };
    let outer = xml
        .root_element()
        .first_element_child()
        .unwrap()
        .first_element_child()
        .unwrap();
    assert_eq!(failure.level().as_str(), "freecad.property_value");
    assert_eq!(
        failure.offset(),
        u64::try_from(outer.range().start).unwrap()
    );
    assert_eq!(
        failure.byte_len(),
        u64::try_from(outer.range().len()).unwrap()
    );
    assert_eq!(output.losses().len(), 1);
    assert_eq!(output.losses()[0].code.local_code(), "record.unreadable");
    let text_start = u64::try_from(document.find(&payload).unwrap()).unwrap();
    let text_end = text_start + u64::try_from(payload.len()).unwrap();
    let captures = ctx.take_record_captures().unwrap();
    let capture = captures
        .iter()
        .find(|capture| capture.id() == property.id)
        .unwrap();
    assert!(capture
        .reads()
        .values()
        .flatten()
        .all(|range| range.end <= text_start || range.start >= text_end));
}

#[test]
fn external_file_links_do_not_become_archive_side_entry_requirements() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::Feature" name="A"/></Objects><ObjectData Count="1"><Object name="A"><Properties Count="1"><Property name="Source" type="App::PropertyXLink"><XLink file="external.FCStd" name="Remote"/></Property></Properties></Object></ObjectData></Document>"#;
    let bytes = crate::test_support::test_archive::archive(document);
    let recovered = crate::FcstdCodec
        .decode(
            &mut std::io::Cursor::new(bytes),
            &cadmpeg_ir::DecodeOptions::default(),
        )
        .unwrap();
    assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
}
