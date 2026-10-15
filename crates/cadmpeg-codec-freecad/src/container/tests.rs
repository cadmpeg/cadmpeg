// SPDX-License-Identifier: Apache-2.0
//! Archive scan and physical-ledger unit tests.

use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::test_archive::{
    archive, archive_entries, streaming_archive, streaming_archive_with_options,
};
use crate::FcstdCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::{Codec, Confidence, DecodeOptions};
use std::io::Cursor;
use zip::write::SimpleFileOptions;

#[test]
fn unreadable_entry_payload_uses_checked_hex() {
    let data: Vec<u8> = (0..=255).cycle().take(256 * 1024).collect();
    let record = super::UnreadableEntry {
        id: "fcstd:native:unreadable_entry#Payload.bin".into(),
        name: "Payload.bin".into(),
        data_start: 0,
        data_end: u64::try_from(data.len()).unwrap(),
        stored_data: data.clone().into(),
        error: "unsupported compression".into(),
    };
    crate::test_support::with_service_context(&[], |ctx| {
        let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
        namespace
            .set_arena(ctx, "unreadable_entries", &[record])
            .unwrap();
        let wire = namespace
            .arena_as::<serde_json::Value>("unreadable_entries")
            .unwrap();
        assert_eq!(
            wire[0]["stored_data"].as_str().unwrap().len(),
            data.len() * 2
        );
        let typed = namespace
            .arena_as::<super::UnreadableEntry>("unreadable_entries")
            .unwrap();
        assert_eq!(typed[0].stored_data, data);
        for payload in [
            serde_json::json!("a"),
            serde_json::json!("gg"),
            serde_json::json!([0]),
        ] {
            let mut invalid = wire[0].clone();
            invalid["stored_data"] = payload;
            assert!(serde_json::from_value::<super::UnreadableEntry>(invalid).is_err());
        }
    });
}

fn with_scanned_document<T>(f: impl FnOnce(&mut super::Scan<'_>) -> T) -> T {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let bytes = archive(document);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("archive is within input limits");
    let mut scan = super::scan(&ctx, root).expect("valid scanned document");
    f(&mut scan)
}

#[test]
fn entry_reference_index_preserves_order_and_deduplicates_property_markers() {
    with_scanned_document(|scan| {
        let properties: Vec<_> = ["First", "Second"]
            .into_iter()
            .map(|name| crate::native::PropertyRecord {
                id: format!("fcstd:native:property#{name}"),
                owner: "fcstd:native:object#Owner".into(),
                name: name.into(),
                type_name: "App::PropertyFileIncluded".into(),
                family: crate::native::PropertyFamily::File,
                status: None,
                body: crate::native::PropertyBody::Persisted {
                    values: Vec::new(),
                    links: Vec::new(),
                    dynamic: None,
                    side_entries: vec![
                        scan.entries[0].name.clone(),
                        "Missing.bin".into(),
                        scan.entries[0].name.clone(),
                    ],
                },
                order: 0,
                xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).unwrap(),
            })
            .collect();
        let entries = super::entry_records(
            &cadmpeg_test_support::service_decode_context(),
            scan,
            &properties,
        )
        .unwrap();
        assert_eq!(
            entries[0].referenced_by(),
            [
                "fcstd:native:property#First",
                "fcstd:native:property#Second"
            ]
        );
    });
}

#[test]
fn entry_data_copy_refuses_at_retained_limit() {
    with_scanned_document(|scan| {
        let name = &scan.entries[0].name;
        scan.data.get(name).expect("entry data");
        crate::test_support::assert_retained_refusal_at(&[], "retain FCStd entry", |ctx| {
            super::entry_records(ctx, scan, &[])
        });
    });
}

#[test]
fn x62_object_envelope_is_admitted_before_the_xml_tree() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="Part::Feature" name="A"/></Objects><ObjectData Count="0"/></Document>"#;
    let bytes = archive(document);
    let mut options = cadmpeg_core::decode::InspectOptions {
        limits: cadmpeg_core::decode::ResourceLimits::service(),
    };
    FcstdCodec
        .inspect(&mut Cursor::new(&bytes), &options)
        .expect("inspect the container");

    options.limits.max_entities = 0;
    let error = FcstdCodec
        .inspect(&mut Cursor::new(&bytes), &options)
        .expect_err("object count must be charged before parsing the tree");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit FCStd document objects"
    ));
}

#[test]
fn x62_prefixed_object_envelope_is_admitted_before_the_xml_tree() {
    let document = r#"<fc:Document xmlns:fc="urn:freecad" SchemaVersion="4"><fc:Objects Count="1"><fc:Object type="Part::Feature" name="A"/></fc:Objects><fc:ObjectData Count="0"/></fc:Document>"#;
    let bytes = archive(document);
    let mut options = cadmpeg_core::decode::InspectOptions {
        limits: cadmpeg_core::decode::ResourceLimits::service(),
    };
    FcstdCodec
        .inspect(&mut Cursor::new(&bytes), &options)
        .expect("inspect the container");

    options.limits.max_entities = 0;
    let error = FcstdCodec
        .inspect(&mut Cursor::new(&bytes), &options)
        .expect_err("prefixed object count must be charged before parsing the tree");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::Entities
                && limit.operation == "admit FCStd document objects"
    ));
}

#[test]
fn xml_envelope_scan_skips_comments_cdata_and_quoted_brackets() {
    let bytes = br#"<Document SchemaVersion="4"><!-- <Objects><Object/> --><Note attr=">"> <![CDATA[<Object/>]]> </Note><Objects><Object/><Object/></Objects></Document>"#;
    let (bound, objects) = super::xml_envelope_counts(bytes).expect("lexical XML count");
    assert_eq!(objects, 2);
    let parsed = roxmltree::Document::parse(std::str::from_utf8(bytes).expect("UTF-8 XML"))
        .expect("XML document");
    assert!(bound >= cadmpeg_core::decode::u64_from_index(parsed.descendants().count()));
}

#[test]
fn frames_zip64_streaming_descriptor_and_local_extra() {
    let bytes = streaming_archive_with_options(
        "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>",
        SimpleFileOptions::default().large_file(true),
    );
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("ZIP64 archive fits root policy");
    let scan = crate::container::scan(&ctx, root).expect("ZIP64 streaming ZIP");
    assert!(scan
        .ledger
        .iter()
        .any(|span| span.role.as_str() == "local-extra" && span.span.end() > span.span.start()));
    let descriptor = scan
        .ledger
        .iter()
        .find(|span| span.role.as_str() == "data-descriptor")
        .expect("ZIP64 descriptor");
    assert_eq!(descriptor.span.end() - descriptor.span.start(), 24);
}

#[test]
fn frames_streaming_data_descriptor_separately_from_padding() {
    let bytes = streaming_archive("<Document SchemaVersion=\"4\" FileVersion=\"1\"/>");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
        .expect("archive fits root policy");
    let scan = crate::container::scan(&ctx, root).expect("streaming ZIP");
    let descriptors = scan
        .ledger
        .iter()
        .filter(|span| span.role.as_str() == "data-descriptor")
        .collect::<Vec<_>>();
    assert_eq!(descriptors.len(), 1);
    assert!(matches!(
        descriptors[0].span.end() - descriptors[0].span.start(),
        16 | 24
    ));
}

#[test]
pub(crate) fn rejects_unsafe_names() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let unsafe_name = archive_entries(&[("../Document.xml", xml), ("Document.xml", xml)]);
    let error = FcstdCodec
        .inspect(
            &mut Cursor::new(unsafe_name),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect_err("unsafe path must fail");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
}

#[test]
fn inspects_and_closes_physical_ledger() {
    let bytes = archive("<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\"><Object/></Document>");
    let archive_len = cadmpeg_core::decode::u64_from_index(bytes.len());
    let summary = FcstdCodec
        .inspect(
            &mut Cursor::new(&bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .expect("inspect");
    assert_eq!(summary.format(), "fcstd");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=4"));
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .expect("decode");
    assert!(result.report().losses.is_empty());
    let ledger = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
        .expect("ledger");
    assert_eq!(ledger.first().map(|span| span.span.start()), Some(0));
    assert_eq!(ledger.last().map(|span| span.span.end()), Some(archive_len));
    assert!(ledger
        .windows(2)
        .all(|pair| pair[0].span.end() == pair[1].span.start()));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    for role in [
        "local-signature",
        "local-fields",
        "local-name",
        "compressed-payload",
        "central-signature",
        "central-fields",
        "central-name",
        "end-record",
    ] {
        assert!(
            ledger.iter().any(|span| span.role.as_str() == role),
            "missing {role}"
        );
    }
}

#[test]
fn decode_refuses_when_max_entities_is_below_object_cardinality() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Feature" name="A" id="1"/>
 <Object type="Part::Feature" name="B" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="A"><Properties Count="0"></Properties></Object>
 <Object name="B"><Properties Count="0"></Properties></Object>
</ObjectData></Document>"#;
    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = FcstdCodec
        .decode(&mut Cursor::new(archive(document)), &options)
        .expect_err("max_entities below document object count must refuse at admission");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
        ),
        "{error:?}"
    );
}

#[test]
fn decode_keeps_document_objects_and_model_entities_additive() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Stored" id="1"/></Objects>
<ObjectData Count="1"><Object name="Stored"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("decode stored feature");
    assert_eq!(decoded.ir().model.entity_count(), 1);

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = FcstdCodec
        .decode(&mut Cursor::new(archive(document)), &options)
        .expect_err("one document object plus one model entity require a limit of two");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit FCStd entities"
        ),
        "{error:?}"
    );

    options.policy.limits.max_entities = 2;
    FcstdCodec
        .decode(&mut Cursor::new(archive(document)), &options)
        .expect("the exact additive entity limit must admit the fixture");
}

#[test]
fn thumbnail_bytes_are_retained_with_digest() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let bytes = archive_entries(&[("Document.xml", xml), ("thumbnails/Thumbnail.png", b"png")]);
    let result = EditableDecodeResult::from(
        FcstdCodec
            .decode(
                &mut Cursor::new(bytes),
                &DecodeOptions {
                    container_only: true,
                    ..DecodeOptions::default()
                },
            )
            .expect("decode"),
    );
    assert_eq!(
        result.ir().native_unknowns_iter("fcstd").count(),
        1,
        "thumbnail has one product reference"
    );
    let retained = result
        .source_fidelity()
        .retained_records()
        .values()
        .next()
        .expect("retained thumbnail");
    assert_eq!(retained.data(), Some(b"png".as_slice()));
}

#[test]
fn retains_every_reference_to_a_shared_side_entry() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Owner"/></Objects>
<ObjectData Count="1"><Object name="Owner"><Properties Count="2">
<Property name="First" type="App::PropertyFileIncluded"><File file="Shared.bin"/></Property>
<Property name="Second" type="App::PropertyFileIncluded"><File file="Shared.bin"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("Shared.bin", b"shared"),
            ])),
            &DecodeOptions::default(),
        )
        .expect("shared side entry");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let shared = entries
        .iter()
        .find(|entry| entry.name() == "Shared.bin")
        .expect("shared entry");
    let spans = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    let span = spans
        .iter()
        .find(|span| span.entry == "Shared.bin")
        .expect("shared entry span");

    assert_eq!(shared.referenced_by().len(), 2);
    assert_ne!(shared.referenced_by()[0], shared.referenced_by()[1]);
    assert_eq!(span.classification.as_str(), "named_opaque");
    assert_eq!(span.classification.owner(), Some(shared.id()));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());

    let mut corrupted = result.ir().clone();
    let mut corrupted_entries = entries.clone();
    let shared = corrupted_entries
        .iter_mut()
        .find(|entry| entry.name() == "Shared.bin")
        .expect("shared entry");
    let mut references = shared.referenced_by().to_vec();
    references.pop();
    *shared = crate::test_support::entry_record(
        shared.id().to_owned(),
        shared.name().to_owned(),
        shared.role,
        references,
        shared.data().to_vec(),
    );
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "entries",
            &corrupted_entries,
        )
        .expect("replace entries");
    assert!(crate::test_support::validate_native(&corrupted)
        .iter()
        .any(|finding| finding.check == cadmpeg_ir::report::check::Check::ReferentialIntegrity));
}

#[test]
fn detects_marker_but_not_arbitrary_zip() {
    assert_eq!(
        cadmpeg_test_support::detection::confidence(
            &FcstdCodec,
            &archive("<Document SchemaVersion=\"4\" FileVersion=\"1\"/>")
        ),
        Confidence::High
    );
    let public = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
    ));
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&FcstdCodec, &public[..512]),
        Confidence::High
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&FcstdCodec, b"PK\x03\x04 unrelated"),
        Confidence::Low
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&FcstdCodec, b"not zip"),
        Confidence::No
    );
}

#[test]
fn producer_version_metadata_does_not_refuse_the_document() {
    for (attributes, expected) in [
        ("programVersion=\"1.0\"", Some("1.0")),
        ("ProgramVersion=\"1.0\" programVersion=\"1.0\"", Some("1.0")),
        ("ProgramVersion=\"1.0\" programVersion=\"2.0\"", Some("1.0")),
    ] {
        let xml = format!(
            "<Document SchemaVersion=\"4\" {attributes}><Objects Count=\"0\"/><ObjectData Count=\"0\"/></Document>"
        );
        let bytes = archive(&xml);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()).unwrap();
        let scan = crate::container::scan(&ctx, root)
            .expect("metadata does not select persistence grammar");
        assert_eq!(scan.document.program_version.as_deref(), expected);
        let summary = crate::container::summarize(&scan);
        assert!(summary.losses.iter().any(|loss| loss.code
            == crate::loss::FreecadLossCode::ProgramVersionNoncanonical
                .note("")
                .code));
        drop(scan);
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert!(decoded.report().losses.iter().any(|loss| loss.code
            == crate::loss::FreecadLossCode::ProgramVersionNoncanonical
                .note("")
                .code));
    }
}

#[test]
fn invalid_file_version_preserves_independent_brep_geometry_and_declaration() {
    use std::io::Read;
    let original = crate::test_support::test_archive::GEOMETRY;
    let expected = FcstdCodec
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .unwrap();
    assert!(!expected.ir().model.faces.is_empty());
    let mut source = zip::ZipArchive::new(Cursor::new(original)).unwrap();
    let mut entries = Vec::new();
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.push((entry.name().to_owned(), bytes));
    }
    for version in ["bad", "-1", "", "184467440737095516160"] {
        let modified = entries
            .iter()
            .map(|(name, bytes)| {
                let bytes = if name == "Document.xml" {
                    String::from_utf8(bytes.clone())
                        .unwrap()
                        .replace("FileVersion=\"1\"", &format!("FileVersion=\"{version}\""))
                        .into_bytes()
                } else {
                    bytes.clone()
                };
                (name.clone(), bytes)
            })
            .collect::<Vec<_>>();
        let borrowed = modified
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        let bytes = archive_entries(&borrowed);
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert!(recovered.report().losses.iter().any(|loss| loss.code
            == crate::loss::FreecadLossCode::FileVersionUnverified
                .note("")
                .code));
        assert_eq!(
            recovered
                .ir()
                .source
                .as_ref()
                .unwrap()
                .dialect()
                .unwrap()
                .declared()["file_version"],
            version
        );
        crate::test_support::test_archive::assert_valid_document(recovered.ir());
    }
}

#[test]
fn equivalent_numeric_schema_declarations_select_the_same_persistence_grammar() {
    use crate::test_support::test_archive::{
        assert_valid_document, rewrite_entry, rewrite_schema_version, GEOMETRY,
    };
    let schema_two = rewrite_entry(GEOMETRY, "Document.xml", |data| {
        let text = std::str::from_utf8(data).unwrap();
        // Schema 2 has no Dependencies attribute and uses the Feature vocabulary.
        let text = text
            .replace("SchemaVersion=\"4\"", "SchemaVersion=\"2\"")
            .replace("ObjectData", "FeatureData")
            .replace("Objects", "Features")
            .replace("<Object ", "<Feature ")
            .replace("</Object>", "</Feature>")
            .replace(" Dependencies=\"1\"", "");
        let xml = roxmltree::Document::parse(&text).unwrap();
        let mut ranges = xml
            .descendants()
            .filter(|node| node.has_tag_name("ObjectDeps"))
            .map(|node| node.range())
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| range.start);
        let mut text = text;
        for range in ranges.into_iter().rev() {
            text.replace_range(range, "");
        }
        text.into_bytes()
    });
    let expected = FcstdCodec
        .decode(&mut Cursor::new(&schema_two), &DecodeOptions::default())
        .unwrap();
    assert!(!expected.ir().model.faces.is_empty());
    for spelling in ["02", "+2", "0002"] {
        let source = rewrite_schema_version(&schema_two, spelling);
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert_valid_document(recovered.ir());
        assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
    }
}
