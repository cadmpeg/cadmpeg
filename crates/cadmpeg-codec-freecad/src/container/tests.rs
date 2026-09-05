// SPDX-License-Identifier: Apache-2.0
//! Archive scan and physical-ledger unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::{Codec, Confidence, DecodeOptions};
use std::io::Cursor;
use zip::write::SimpleFileOptions;

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
        .any(|span| span.role.as_str() == "local-extra" && span.end > span.start));
    let descriptor = scan
        .ledger
        .iter()
        .find(|span| span.role.as_str() == "data-descriptor")
        .expect("ZIP64 descriptor");
    assert_eq!(descriptor.end - descriptor.start, 24);
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
    assert!(matches!(descriptors[0].end - descriptors[0].start, 16 | 24));
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
    assert_eq!(ledger.first().map(|span| span.start), Some(0));
    assert_eq!(ledger.last().map(|span| span.end), Some(archive_len));
    assert!(ledger.windows(2).all(|pair| pair[0].end == pair[1].start));
    assert!(crate::validate_native(result.ir()).is_empty());
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
                    && limit.context.operation == "admit FCStd entities"
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
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
                ..DecodeOptions::default()
            },
        )
        .expect("decode");
    assert_eq!(
        result.ir().native_unknowns_iter("fcstd").count(),
        1,
        "thumbnail has one product reference"
    );
    let retained = result
        .source_fidelity()
        .retained_records
        .first()
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
    assert!(crate::validate_native(result.ir()).is_empty());

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
        .namespace_mut("fcstd", std::num::NonZeroU32::MIN)
        .set_arena("entries", &corrupted_entries)
        .expect("replace entries");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding.check == cadmpeg_ir::Check::ReferentialIntegrity));
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
