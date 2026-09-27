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
fn document_root_error_refuses_at_retained_limit() {
    let bytes = b"<UnexpectedRoot SchemaVersion=\"4\"/>";
    crate::test_support::assert_retained_refusal_at(&[], "FCStd document root error", |ctx| {
        super::parse_document(ctx, bytes)
    });
}

#[test]
fn document_parse_error_refuses_at_retained_limit() {
    let bytes = b"<Document>";
    crate::test_support::assert_retained_refusal_at(&[], "FCStd document parse error", |ctx| {
        super::parse_document(ctx, bytes)
    });
}

#[test]
fn missing_entry_error_refuses_at_retained_limit() {
    with_scanned_document(|scan| {
        scan.data.clear();
        crate::test_support::assert_retained_refusal_at(&[], "FCStd missing entry error", |ctx| {
            super::entry_records(ctx, scan, &[])
        });
    });
}

#[test]
fn overlapping_logical_span_error_refuses_at_retained_limit() {
    let entry = crate::native::EntryRecord {
        id: "fcstd:native:entry#Document.xml".into(),
        name: "Document.xml".into(),
        role: cadmpeg_core::container::ContainerRole::Auxiliary,
        referenced_by: Vec::new(),
        data: vec![0; 11],
    };
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#One".into(),
        owner: "fcstd:native:object#Owner".into(),
        name: "One".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let properties = [property.clone(), property];
    crate::test_support::assert_retained_refusal_at(&[], "FCStd logical span error", |ctx| {
        super::logical_ledger(ctx, &[entry.clone()], &properties, &crate::gui::Graph::default(), &[], &[], &[])
    });
}

fn collection_context<T>(limit: u64, f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within input limits");
    f(&ctx)
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
fn archive_span_identity_refuses_at_retained_limit() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let bytes = archive(document);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    for _ in 0..256 {
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("archive root");
        let error = super::scan(&ctx, root).err().expect("retained limit must refuse");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("expected retained refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        let threshold = limit.used.checked_add(limit.additional).expect("finite test budget");
        if limit.operation == "FreeCAD native identity" {
            policy.limits.max_retained_bytes = threshold - 1;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("archive root");
            assert!(matches!(super::scan(&ctx, root),
                Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                    if refusal.operation == "FreeCAD native identity"));
            return;
        }
        policy.limits.max_retained_bytes = threshold;
    }
    panic!("archive span identity was not reached");
}

#[test]
fn source_domain_list_refuses_at_retained_limit() {
    with_scanned_document(|scan| {
        scan.document.domains.push("Part".into());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = "Part".len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::source_attributes(&ctx, scan),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FCStd source domain list"));
    });
}

#[test]
fn source_program_version_refuses_at_retained_limit() {
    with_scanned_document(|scan| {
        scan.document.program_version = Some("1.2.3".into());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = "1.2.3".len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::source_attributes(&ctx, scan),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FCStd source program version"));
    });
}

#[test]
fn entry_record_vector_refuses_at_collection_limit() {
    with_scanned_document(|scan| {
        collection_context(0, |ctx| {
            assert!(matches!(super::entry_records(ctx, scan, &[]),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.operation == "FCStd entry records"));
        });
    });
}

#[test]
fn entry_referencing_property_refuses_at_collection_limit() {
    with_scanned_document(|scan| {
        let entry_name = &scan.entries[0].name;
        let property = crate::native::PropertyRecord {
            id: "fcstd:native:property#Entry".into(),
            owner: "fcstd:native:object#Owner".into(),
            name: "Entry".into(),
            type_name: "App::PropertyFileIncluded".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Persisted {
                values: Vec::new(),
                links: Vec::new(),
                side_entries: vec![entry_name.clone()],
                dynamic: None,
            },
            order: 0,
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
                .expect("valid XML span"),
        };
        collection_context(scan.entries.len() as u64, |ctx| {
            assert!(matches!(super::entry_records(ctx, scan, &[property]),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.operation == "FCStd entry referencing properties"));
        });
    });
}

#[test]
fn entry_referencing_identity_refuses_at_retained_limit() {
    with_scanned_document(|scan| {
        let property = crate::native::PropertyRecord {
            id: "fcstd:native:property#Entry".into(),
            owner: "fcstd:native:object#Owner".into(),
            name: "Entry".into(),
            type_name: "App::PropertyFileIncluded".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: crate::native::PropertyBody::Persisted {
                values: Vec::new(),
                links: Vec::new(),
                side_entries: vec![scan.entries[0].name.clone()],
                dynamic: None,
            },
            order: 0,
            xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
                .expect("valid XML span"),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = property.id.len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::entry_records(&ctx, scan, &[property]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FCStd entry referencing identity"));
    });
}

#[test]
fn entry_identity_refuses_at_retained_limit() {
    with_scanned_document(|scan| {
        let name = &scan.entries[0].name;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = crate::native::native_id("entry", name).len() as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::entry_records(&ctx, scan, &[]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FreeCAD native identity"));
    });
}

#[test]
fn entry_name_copy_refuses_at_retained_limit() {
    with_scanned_document(|scan| {
        let name = &scan.entries[0].name;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = (crate::native::native_id("entry", name).len() + name.len()) as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::entry_records(&ctx, scan, &[]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FCStd entry record name"));
    });
}

#[test]
fn entry_data_copy_refuses_at_retained_limit() {
    with_scanned_document(|scan| {
        let name = &scan.entries[0].name;
        let byte_len = scan.data.get(name).expect("entry data").window().len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = (crate::native::native_id("entry", name).len() + name.len() + byte_len) as u64 - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root is within policy");
        assert!(matches!(super::entry_records(&ctx, scan, &[]),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "retain FCStd entry"));
    });
}

fn resource_entry_record() -> crate::native::EntryRecord {
    crate::native::EntryRecord {
        id: "fcstd:native:entry#GuiDocument.xml".into(),
        name: "GuiDocument.xml".into(),
        role: cadmpeg_core::container::ContainerRole::GuiDocument,
        referenced_by: Vec::new(),
        data: Vec::new(),
    }
}

#[test]
fn gui_entry_reference_refuses_at_collection_limit() {
    collection_context(0, |ctx| {
        let mut entry = resource_entry_record();
        assert!(matches!(super::add_entry_reference(ctx, &mut entry, "owner"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "FCStd GUI entry references"));
    });
}

#[test]
fn gui_entry_reference_identity_refuses_at_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = "owner".len() as u64 - 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    let mut entry = resource_entry_record();
    assert!(matches!(super::add_entry_reference(&ctx, &mut entry, "owner"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FCStd GUI entry reference identity"));
}

#[test]
fn document_domain_set_refuses_on_collection_limit() {
    let document = b"<Document SchemaVersion=\"4\"><Objects><Object type=\"Part::Feature\"/></Objects></Document>";
    let result = collection_context(0, |ctx| super::parse_document(ctx, document));
    assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "FCStd document domains"));
}

#[test]
fn logical_span_vector_refuses_on_collection_limit() {
    let entry = crate::native::EntryRecord {
        id: "fcstd:native:entry#extra".to_owned(),
        name: "extra".to_owned(),
        role: cadmpeg_core::container::ContainerRole::Auxiliary,
        referenced_by: Vec::new(),
        data: vec![0],
    };
    let result = collection_context(0, |ctx| super::logical_ledger(
        ctx,
        &[entry],
        &[],
        &crate::gui::Graph::default(),
        &[],
        &[],
        &[],
    ));
    assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "FCStd logical ledger spans"));
}

#[test]
fn typed_entry_set_refuses_on_collection_limit() {
    let payload = crate::brep::ShapePayloadRecord {
        id: "payload".to_owned(),
        property: "property".to_owned(),
        entry: "entry".to_owned(),
        payload: crate::brep::ShapePayload::Empty,
    };
    let result = collection_context(0, |ctx| super::logical_ledger(
        ctx,
        &[],
        &[],
        &crate::gui::Graph::default(),
        &[payload],
        &[],
        &[],
    ));
    assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "FCStd typed entry identities"));
}

#[test]
fn ordered_physical_span_vector_refuses_on_collection_limit() {
    let physical = [crate::native::ArchiveSpan {
        id: "span".to_owned(),
        span: crate::native::ByteSpan::try_new(0, 1).expect("nonempty span"),
        role: crate::native::ArchiveSpanRole::Zip64EndRecord,
    }];
    let result = collection_context(0, |ctx| super::byte_coverage(ctx, &physical, &[], &[], 1));
    assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "FCStd ordered physical spans"));
}

#[test]
fn coverage_classification_map_refuses_on_collection_limit() {
    let logical = [crate::native::LogicalSpan {
        id: "logical".to_owned(),
        entry: "extra".to_owned(),
        span: crate::native::ByteSpan::try_new(0, 1).expect("nonempty span"),
        classification: crate::native::LogicalClassification::Structural,
    }];
    let result = collection_context(0, |ctx| super::byte_coverage(ctx, &[], &[], &logical, 0));
    assert!(matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "FCStd coverage classifications"));
}

#[test]
fn summary_schema_note_refuses_on_retained_limit() {
    let bytes = archive("<Document SchemaVersion=\"4\" FileVersion=\"1\"/>");
    let scan_arena = DecodeArena::new();
    let scan_policy = DecodePolicy::default();
    let (scan_ctx, root) = DecodeContext::from_root_bytes(&bytes, &scan_arena, &scan_policy)
        .expect("archive fits input policy");
    let scan = super::scan(&scan_ctx, root).expect("valid archive scan");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within input limits");
    assert!(matches!(super::summary_notes(&ctx, &scan), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "FCStd schema note"));
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
        .expect("service profile admits the XML envelope");

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
fn x62_xml_tree_items_are_admitted_before_allocation() {
    let document = r#"<Document SchemaVersion="4"><Objects Count="1"><Object type="Part::Feature" name="A"/></Objects><ObjectData Count="0"/></Document>"#;
    let bytes = archive(document);
    let mut options = cadmpeg_core::decode::InspectOptions {
        limits: cadmpeg_core::decode::ResourceLimits::service(),
    };
    FcstdCodec
        .inspect(&mut Cursor::new(&bytes), &options)
        .expect("service profile admits the XML tree");

    // The ZIP snapshot and FCStd entry table admit six items before the XML tree.
    options.limits.max_collection_items = 6;
    let error = FcstdCodec
        .inspect(&mut Cursor::new(&bytes), &options)
        .expect_err("XML nodes must be charged before parsing the tree");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "FCStd Document.xml node tree"
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
        .expect("service profile admits the prefixed XML envelope");

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
    assert!(bound >= parsed.descendants().count() as u64);
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
    let archive_len = bytes.len() as u64;
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
        .find(|entry| entry.name == "Shared.bin")
        .expect("shared entry");
    let spans = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    let span = spans
        .iter()
        .find(|span| span.entry == "Shared.bin")
        .expect("shared entry span");

    assert_eq!(shared.referenced_by.len(), 2);
    assert_ne!(shared.referenced_by[0], shared.referenced_by[1]);
    assert_eq!(span.classification.as_str(), "named_opaque");
    assert_eq!(span.classification.owner(), Some(shared.id.as_str()));
    assert!(crate::test_support::validate_native(result.ir()).is_empty());

    let mut corrupted = result.ir().clone();
    let mut corrupted_entries = entries.clone();
    corrupted_entries
        .iter_mut()
        .find(|entry| entry.name == "Shared.bin")
        .expect("shared entry")
        .referenced_by
        .pop();
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
        FcstdCodec.detect(&archive(
            "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>"
        )),
        Confidence::High
    );
    let public = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
    ));
    assert_eq!(FcstdCodec.detect(&public[..512]), Confidence::High);
    assert_eq!(FcstdCodec.detect(b"PK\x03\x04 unrelated"), Confidence::Low);
    assert_eq!(FcstdCodec.detect(b"not zip"), Confidence::No);
}
