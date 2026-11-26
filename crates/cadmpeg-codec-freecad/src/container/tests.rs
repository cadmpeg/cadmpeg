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
            &cadmpeg_core::decode::test_support::service_decode_context(),
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

/// A member of a compression method the decoder has no reading for gives up
/// as one `record.undecoded`, which a pinned decode accepts.
#[test]
fn a_member_of_an_unsupported_compression_method_gives_up_as_undecoded() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};

    const BZIP2: u16 = 12;
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document.as_bytes()),
        RawEntry::compressed("thumbnails/Thumbnail.png", BZIP2, b"BZh9", b"thumbnail"),
    ]);
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &options)
        .expect("a pinned decode accepts an undecoded member");
    let given_up: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| cadmpeg_ir::unknown::part::gives_up(&loss.code))
        .collect();
    let [loss] = given_up.as_slice() else {
        panic!("one give-up: {given_up:?}");
    };
    assert_eq!(
        loss.code,
        cadmpeg_ir::unknown::part::record_undecoded_kind()
    );
    assert!(
        loss.message
            .starts_with("part archive-entry \"thumbnails/Thumbnail.png\""),
        "{}",
        loss.message
    );
}

#[test]
fn zstd_member_past_the_window_allowance_is_an_undecoded_entry_without_a_refusal() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};

    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    // The thumbnail is a streamed frame as a level-22 encoder writes it: a
    // 128 MiB window and no content size. Laid out by `zip_fixture`: `zip` is
    // built without its `zstd` feature.
    let bytes = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document.as_bytes()),
        RawEntry::compressed(
            "thumbnails/Thumbnail.png",
            zip_fixture::ZSTD,
            &zip_fixture::zstd_streamed_frame(27, b"thumbnail"),
            b"thumbnail",
        ),
    ]);

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
    let scan = super::scan(&ctx, root).expect("one unreadable member does not refuse the archive");
    assert!(scan.losses.is_empty(), "{:?}", scan.losses);
    let given_up = scan
        .unreadable
        .into_iter()
        .map(|entry| entry.give_up(&ctx, None))
        .collect::<Result<Vec<_>, _>>()
        .expect("the entry is given up");
    let [given_up] = given_up.as_slice() else {
        panic!("one given-up entry");
    };
    let loss = given_up.loss();
    assert_eq!(loss.code.to_string(), "decode/record.undecoded");
    assert_eq!(loss.severity, cadmpeg_ir::report::Severity::Warning);
    assert!(
        loss.message
            .starts_with("part archive-entry \"thumbnails/Thumbnail.png\" stream=\"\" offset=")
            && loss.message.contains("per-expansion allowance")
            && loss.message.ends_with("; dropped=[]"),
        "{}",
        loss.message
    );
    assert!(scan.unreadable_names.contains("thumbnails/Thumbnail.png"));
    assert!(scan.data.contains_key("Document.xml"));
    assert_eq!(ctx.resource_refusal(), None);
    assert!(ctx.finish_session().is_ok());

    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .expect("the decode keeps the document");
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/record.undecoded")
        .collect();
    assert_eq!(losses.len(), 1);
    let (ir, _, fidelity) = decoded.into_parts();
    let stub = ir
        .native_unknowns("fcstd")
        .expect("native unknowns")
        .into_iter()
        .find(|stub| stub.id == *given_up.id())
        .expect("the given-up entry has a stub");
    let [link] = stub.links.as_slice() else {
        panic!("one link: {:?}", stub.links);
    };
    let ledger = ir
        .native
        .namespace("fcstd")
        .expect("native graph")
        .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
        .expect("physical ledger");
    let span = ledger
        .iter()
        .find(|span| span.id == link.as_str())
        .expect("the stub links a ledger span");
    assert!(matches!(&span.role,
        crate::native::ArchiveSpanRole::CompressedPayload(entry)
            if entry == "thumbnails/Thumbnail.png"));
    let retained = fidelity
        .retained_record(given_up.id().as_str())
        .expect("the entry's stored bytes are retained");
    let stored = &bytes[usize::try_from(retained.offset()).expect("offset")..]
        [..usize::try_from(retained.byte_len()).expect("length")];
    assert_eq!(retained.data(), Some(stored));
    assert!(stored
        .windows(4)
        .any(|window| window == [0x28, 0xb5, 0x2f, 0xfd]));
}

/// An archive whose deflated `Document.xml` nests `depth` elements below
/// `Document`, beside stored GUI, side, and thumbnail entries.
fn archive_with_deflated_document(depth: usize) -> (Vec<u8>, usize) {
    use std::io::Write;
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/>{}{}</Document>"#,
        "<Nest>".repeat(depth),
        "</Nest>".repeat(depth)
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "Document.xml",
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    zip.write_all(document.as_bytes()).unwrap();
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        (
            "GuiDocument.xml",
            b"<Document SchemaVersion=\"1\"/>".as_slice(),
        ),
        ("Shared.bin", b"shared"),
        ("thumbnails/Thumbnail.png", b"png"),
    ] {
        zip.start_file(name, stored).unwrap();
        zip.write_all(data).unwrap();
    }
    (zip.finish().unwrap().into_inner(), document.len())
}

#[test]
fn a_part_refusal_on_document_xml_gives_up_that_entry_and_keeps_the_others() {
    use cadmpeg_core::decode::ArenaClass;
    let (bytes, document_len) = archive_with_deflated_document(100);
    let mut past_expansion = DecodeOptions::default();
    past_expansion
        .policy
        .limits
        .max_decompressed_bytes_per_expand = cadmpeg_core::decode::u64_from_index(document_len - 1);
    let mut past_depth = DecodeOptions::default();
    past_depth.policy.limits.max_recursion_depth = 64;
    for (case, options) in [("expansion", past_expansion), ("depth", past_depth)] {
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(&bytes), &options)
            .unwrap_or_else(|error| panic!("{case}: a Part refusal refuses no file: {error:?}"));
        let losses: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.to_string() == "decode/record.undecoded")
            .map(|loss| loss.message.clone())
            .collect();
        let [message] = losses.as_slice() else {
            panic!("{case}: one record.undecoded loss: {losses:?}");
        };
        assert!(
            message.starts_with("part archive-entry \"Document.xml\" stream=\"\" offset=")
                && message.contains("scope Part"),
            "{case}: {message}"
        );
        assert!(decoded.ir().source.is_none(), "{case}");
        assert_eq!(decoded.report().format(), "fcstd", "{case}");
        let namespace = decoded.ir().native.namespace("fcstd").expect("namespace");
        assert!(!namespace.arenas().contains_key("document"), "{case}");
        let mut entries = namespace
            .arena_as::<crate::native::EntryRecord>("entries")
            .expect("entries")
            .iter()
            .map(|entry| (entry.name().to_owned(), entry.data().to_vec()))
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(
            entries,
            [
                (
                    "GuiDocument.xml".to_owned(),
                    b"<Document SchemaVersion=\"1\"/>".to_vec()
                ),
                ("Shared.bin".to_owned(), b"shared".to_vec()),
                ("thumbnails/Thumbnail.png".to_owned(), b"png".to_vec()),
            ],
            "{case}"
        );
        assert!(
            crate::test_support::validate_native(decoded.ir()).is_empty(),
            "{case}: {:?}",
            crate::test_support::validate_native(decoded.ir())
        );
        crate::test_support::test_archive::assert_valid_document(decoded.ir());
        let ledger = namespace
            .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
            .expect("physical ledger");
        let (ir, _, fidelity) = decoded.into_parts();
        let [stub] = ir
            .native_unknowns("fcstd")
            .expect("stub")
            .try_into()
            .unwrap();
        assert!(
            message.contains(&format!(" record={}: ", stub.id)),
            "{case}"
        );
        let [link] = stub.links.as_slice() else {
            panic!("{case}: one link: {:?}", stub.links);
        };
        assert!(ledger.iter().any(|span| span.id == link.as_str()
            && matches!(&span.role,
                crate::native::ArchiveSpanRole::CompressedPayload(entry) if entry == "Document.xml")));
        let retained = fidelity
            .retained_record(stub.id.as_str())
            .expect("Document.xml's stored bytes are retained");
        let start = usize::try_from(retained.offset()).unwrap();
        let end = usize::try_from(retained.end_offset()).unwrap();
        assert_eq!(retained.data(), Some(&bytes[start..end]), "{case}");
        assert_eq!(
            retained.untyped(),
            &ArenaClass::ALL.into_iter().collect(),
            "{case}"
        );
        assert_eq!(
            fidelity.unrepresented(),
            ArenaClass::ALL.into_iter().collect(),
            "{case}"
        );

        let summary = FcstdCodec
            .inspect(
                &mut Cursor::new(&bytes),
                &cadmpeg_core::decode::InspectOptions {
                    limits: options.policy.limits,
                },
            )
            .unwrap_or_else(|error| panic!("{case}: inspect: {error:?}"));
        assert!(summary.dialects().is_none(), "{case}");
        assert!(
            summary
                .losses
                .iter()
                .any(|loss| loss.code.to_string() == "decode/record.undecoded"),
            "{case}"
        );
    }
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
                depth: cadmpeg_core::decode::DecodeDepth::Container,
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
    assert_eq!(decoded.ir().entity_count(), 2);

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 2;
    let error = FcstdCodec
        .decode(&mut Cursor::new(archive(document)), &options)
        .expect_err("one document object plus two model entities require a limit of three");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "root product definition"
        ),
        "{error:?}"
    );

    options.policy.limits.max_entities = 3;
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
                    depth: cadmpeg_core::decode::DecodeDepth::Container,
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
    let attributes = &result.ir().source.as_ref().expect("source").attributes;
    for key in [
        "thumbnail_bytes",
        "archive_entry_count",
        "physical_ledger_spans",
        "physical_archive_bytes",
        "object_count",
    ] {
        assert!(!attributes.keys().any(|name| name.as_str() == key), "{key}");
    }
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
        .namespace_mut(cadmpeg_ir::codec_id!("fcstd"))
        .set_arena(
            &cadmpeg_core::decode::test_support::service_decode_context(),
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
        assert_eq!(
            scan.persistence
                .as_ref()
                .and_then(|persistence| persistence.document.program_version.as_deref()),
            expected
        );
        let summary = crate::container::summarize(&ctx, scan).expect("summary");
        assert!(summary.losses.iter().any(|loss| loss.code
            == crate::loss::FreecadLossCode::ProgramVersionNoncanonical
                .note("")
                .code));
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
    assert!(!expected.ir().geometry.faces.is_empty());
    let mut source = zip::ZipArchive::new(Cursor::new(original)).unwrap();
    let mut entries = Vec::new();
    for index in 0..source.len() {
        let mut entry = source.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        entries.push((entry.name().expect("entry name").into_owned(), bytes));
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
        assert_eq!(recovered.ir().geometry, expected.ir().geometry);
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
    assert!(!expected.ir().geometry.faces.is_empty());
    for spelling in ["02", "+2", "0002"] {
        let source = rewrite_schema_version(&schema_two, spelling);
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(recovered.ir().geometry, expected.ir().geometry);
        assert_eq!(recovered.ir().model, expected.ir().model);
        assert_valid_document(recovered.ir());
        assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
    }
}

#[test]
fn byte_coverage_checks_each_entry_that_shares_a_name_against_its_spans() {
    use crate::native::{ByteSpan, EntryRecord, LogicalClassification, LogicalSpan};
    let entry = |id: &str| {
        EntryRecord::new(
            id.into(),
            "dup".into(),
            cadmpeg_core::container::ContainerRole::Auxiliary,
            Vec::new(),
            vec![1, 2, 3],
        )
        .unwrap()
    };
    let entries = [entry("fcstd:native:entry#a"), entry("fcstd:native:entry#b")];
    let logical = [LogicalSpan {
        id: "fcstd:native:logical-span#0".into(),
        entry: "dup".into(),
        span: ByteSpan::try_new(0, 3).unwrap(),
        classification: LogicalClassification::Structural,
    }];
    let physical = [crate::native::ArchiveSpan {
        id: "fcstd:native:archive-span#0".into(),
        span: ByteSpan::try_new(0, 10).unwrap(),
        role: crate::native::ArchiveSpanRole::LocalName("dup".into()),
    }];
    assert!(super::byte_coverage(&physical, &entries[..1], &logical, 10).exact);
    assert!(super::byte_coverage(&physical, &entries, &logical, 10).exact);
}

#[test]
fn byte_coverage_is_inexact_when_a_span_leaves_a_gap_overlaps_or_names_no_entry() {
    use crate::native::{
        ArchiveSpan, ArchiveSpanRole, ByteSpan, EntryRecord, LogicalClassification, LogicalSpan,
    };
    let entry = |name: &str, len: usize| {
        EntryRecord::new(
            format!("fcstd:native:entry#{name}"),
            name.into(),
            cadmpeg_core::container::ContainerRole::Auxiliary,
            Vec::new(),
            vec![0; len],
        )
        .unwrap()
    };
    let logical = |spans: &[(&str, u64, u64)]| {
        spans
            .iter()
            .enumerate()
            .map(|(index, (entry, start, end))| LogicalSpan {
                id: format!("fcstd:native:logical-span#{index}"),
                entry: (*entry).into(),
                span: ByteSpan::try_new(*start, *end).unwrap(),
                classification: LogicalClassification::Structural,
            })
            .collect::<Vec<_>>()
    };
    let physical = |spans: &[(u64, u64)]| {
        spans
            .iter()
            .enumerate()
            .map(|(index, (start, end))| ArchiveSpan {
                id: format!("fcstd:native:archive-span#{index}"),
                span: ByteSpan::try_new(*start, *end).unwrap(),
                role: ArchiveSpanRole::LocalName("a".into()),
            })
            .collect::<Vec<_>>()
    };
    let entries = [entry("a", 4), entry("b", 2), entry("empty", 0)];
    let whole = physical(&[(0, 6), (6, 10)]);
    let exact = |physical: &[ArchiveSpan], spans: &[(&str, u64, u64)]| {
        super::byte_coverage(physical, &entries, &logical(spans), 10).exact
    };

    assert!(exact(&whole, &[("a", 0, 2), ("a", 2, 4), ("b", 0, 2)]));
    assert!(
        exact(&whole, &[("b", 0, 2), ("a", 2, 4), ("a", 0, 2)]),
        "spans are ordered by start before they are checked"
    );
    assert!(
        exact(
            &physical(&[(6, 10), (0, 6)]),
            &[("a", 0, 2), ("a", 2, 4), ("b", 0, 2)]
        ),
        "archive spans are ordered by start before they are checked"
    );
    for (case, spans) in [
        ("gap", &[("a", 0, 1), ("a", 2, 4), ("b", 0, 2)][..]),
        ("overlap", &[("a", 0, 3), ("a", 2, 4), ("b", 0, 2)][..]),
        ("late start", &[("a", 1, 4), ("b", 0, 2)][..]),
        ("short end", &[("a", 0, 3), ("b", 0, 2)][..]),
        ("long end", &[("a", 0, 5), ("b", 0, 2)][..]),
        ("entry without spans", &[("a", 0, 4)][..]),
        (
            "missing entry",
            &[("a", 0, 4), ("b", 0, 2), ("c", 0, 1)][..],
        ),
        (
            "span in an empty entry",
            &[("a", 0, 4), ("b", 0, 2), ("empty", 0, 1)][..],
        ),
    ] {
        assert!(!exact(&whole, spans), "{case}");
    }

    let spans = [("a", 0, 4), ("b", 0, 2)];
    for (case, archive) in [
        ("archive gap", physical(&[(0, 5), (6, 10)])),
        ("archive overlap", physical(&[(0, 7), (6, 10)])),
        ("archive late start", physical(&[(1, 10)])),
        ("archive short of its length", physical(&[(0, 9)])),
        ("archive past its length", physical(&[(0, 11)])),
        ("no archive spans", Vec::new()),
    ] {
        assert!(!exact(&archive, &spans), "{case}");
    }
}
