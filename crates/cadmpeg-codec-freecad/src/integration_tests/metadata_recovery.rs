// SPDX-License-Identifier: Apache-2.0
//! Independent persistence metadata, source retention, and complete codec checks.

use crate::native::PropertyRecord;
use crate::test_support::test_archive::{archive, archive_entries};
use crate::FcstdCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::fmt::Write as _;
use std::io::Cursor;

#[test]
fn damaged_first_optional_frame_preserves_detection_and_the_required_document() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let original = archive_entries(&[
        ("Thumbnail.png", b"preview"),
        ("Document.xml", document.as_bytes()),
    ]);
    let mut changed = original.clone();
    changed[0] ^= 1;
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&FcstdCodec, &changed),
        cadmpeg_ir::Confidence::Medium
    );
    let baseline = FcstdCodec
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .expect("baseline document");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(changed), &DecodeOptions::default())
        .expect("required document remains readable");
    assert_eq!(decoded.ir().model, baseline.ir().model);
    assert!(!decoded.report().losses.is_empty());
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());

    let mut unrelated = archive_entries(&[("Thumbnail.png", b"Document.xml")]);
    unrelated[0] ^= 1;
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&FcstdCodec, &unrelated),
        cadmpeg_ir::Confidence::No
    );
}

#[test]
fn damaged_late_optional_frame_validates_in_identity_sorted_native_arenas() {
    let mut original = crate::test_support::test_archive::GEOMETRY.to_vec();
    // Place the damaged member beyond span 99, the last lexicographic ID.
    for ordinal in 0..24 {
        original = crate::test_support::test_archive::rewrite_entry(
            &original,
            &format!("auxiliary-{ordinal}.bin"),
            |_| b"ancillary".to_vec(),
        );
    }
    let original =
        crate::test_support::test_archive::rewrite_entry(&original, "Thumbnail.png", |_| {
            b"preview".to_vec()
        });
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&original, &arena, &policy).unwrap();
    let snapshot = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let entry = snapshot.entry("Thumbnail.png").unwrap();
    let mut changed = original.clone();
    changed[usize::try_from(entry.header_start).unwrap()] ^= 1;
    let baseline = FcstdCodec
        .decode(&mut Cursor::new(original), &DecodeOptions::default())
        .unwrap();
    assert!(!baseline.ir().model.faces.is_empty());
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().model, baseline.ir().model);
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    crate::test_support::test_archive::assert_valid_document(decoded.ir());
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let unreadable = namespace
        .arena_as::<crate::container::UnreadableEntry>("unreadable_entries")
        .unwrap();
    assert_eq!(unreadable.len(), 1);
    let physical = namespace
        .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
        .unwrap();
    assert!(physical.last().unwrap().span.end() < unreadable[0].data_end);
    let start = usize::try_from(unreadable[0].data_start).unwrap();
    let end = usize::try_from(unreadable[0].data_end).unwrap();
    assert_eq!(unreadable[0].stored_data, changed[start..end]);

    let mut shifted = unreadable;
    shifted[0].data_start += 1;
    shifted[0].data_end += 1;
    let mut invalid = decoded.ir().clone();
    invalid
        .native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "unreadable_entries",
            &shifted,
        )
        .unwrap();
    assert!(crate::test_support::validate_native(&invalid)
        .iter()
        .any(|finding| finding.message == "invalid source-only FCStd entry"));
}

#[test]
fn unreadable_optional_payload_retains_stored_bytes_and_checked_geometry() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let original = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Thumbnail.png", b"preview"),
    ]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&original, &arena, &policy).expect("root");
    let snapshot = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).expect("index");
    let file = snapshot.entry("Thumbnail.png").expect("thumbnail");
    let payload = usize::try_from(file.data_start.expect("readable frame")).expect("offset");
    let mut changed = original.clone();
    changed[payload] ^= 1;
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
        .expect("independent document");
    assert!(crate::validate_native(
        &cadmpeg_test_support::service_decode_context(),
        decoded.ir()
    )
    .expect("native validation")
    .is_empty());
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("neutral validation")
        .is_ok());
    let namespace = decoded
        .ir()
        .native
        .namespace("fcstd")
        .expect("native graph");
    let raw = namespace
        .arena_as::<crate::container::UnreadableEntry>("unreadable_entries")
        .expect("source-only payload");
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0].name, "Thumbnail.png");
    assert!(!raw[0].stored_data.is_empty());
    let admitted = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("admitted entries");
    assert!(!admitted.iter().any(|entry| entry.name() == "Thumbnail.png"));
}

#[test]
fn empty_unreadable_payload_still_requires_its_physical_source_entry() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let mut bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Thumbnail.png", b""),
    ]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let snapshot = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let entry = snapshot.entry("Thumbnail.png").unwrap();
    let local = usize::try_from(entry.header_start).unwrap();
    let central = usize::try_from(entry.central_start).unwrap();
    bytes[local + 8..local + 10].copy_from_slice(&12_u16.to_le_bytes());
    bytes[central + 10..central + 12].copy_from_slice(&12_u16.to_le_bytes());
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    let mut source = result
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::container::UnreadableEntry>("unreadable_entries")
        .unwrap();
    assert!(source[0].stored_data.is_empty());
    source[0].name = "Absent.png".into();
    source[0].id = "fcstd:native:unreadable_entry#Absent.png".into();
    let mut changed = result.ir().clone();
    changed
        .native
        .namespace_mut("fcstd")
        .set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "unreadable_entries",
            &source,
        )
        .unwrap();
    assert!(crate::test_support::validate_native(&changed)
        .iter()
        .any(|finding| finding.message == "invalid source-only FCStd entry"));
}

#[test]
fn unrelated_missing_attachments_recover_but_shape_sidecars_remain_required() {
    for (type_name, payload, required) in [
        ("App::PropertyFile", "<File name=\"notes.txt\"/>", false),
        (
            "Part::PropertyPartShape",
            "<Part file=\"shape.brp\"/>",
            true,
        ),
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1"><Properties Count="1"><Property name="Payload" type="{type_name}">{payload}</Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#
        );
        let result = FcstdCodec.decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        );
        if required {
            assert!(result.is_err());
        } else {
            let decoded = result.expect("missing unrelated attachment");
            assert!(decoded.report().losses.iter().any(|loss| loss.code
                == crate::loss::FreecadLossCode::PersistenceSideEntryUnresolved
                    .note(String::new())
                    .code));
            assert!(crate::validate_native(
                &cadmpeg_test_support::service_decode_context(),
                decoded.ir()
            )
            .expect("validation")
            .is_empty());
        }
    }
}

#[test]
fn large_property_population_preserves_the_complete_checked_model() {
    let source = crate::test_support::test_archive::GEOMETRY;
    let bytes = crate::test_support::test_archive::rewrite_entry(source, "Document.xml", |data| {
        let document = std::str::from_utf8(data).unwrap();
        let mut addition = String::new();
        for index in 0..32_000 {
            writeln!(addition, r#"<Property name="Stress{index}" type="App::PropertyString"><String value=""/></Property>"#).unwrap();
        }
        document
            .replacen("<Properties Count=\"17\"", "<Properties Count=\"32017\"", 1)
            .replacen("</Properties>", &(addition + "</Properties>"), 1)
            .into_bytes()
    });
    let options = DecodeOptions::default();
    let baseline = crate::FcstdCodec
        .decode(&mut Cursor::new(source), &options)
        .unwrap();
    let result = crate::FcstdCodec
        .decode(&mut Cursor::new(&bytes), &options)
        .unwrap();
    assert_eq!(result.ir().model, baseline.ir().model);
    crate::test_support::test_archive::assert_valid_document(result.ir());
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &options.policy).unwrap();
    assert!(crate::validate_native(&ctx, result.ir())
        .unwrap()
        .is_empty());
    let properties = result.ir().native.namespace("fcstd").unwrap().arenas();
    assert!(properties["properties"].len() >= 32_000);
}

#[test]
fn dependency_and_extension_count_metadata_preserves_checked_geometry() {
    let source = crate::test_support::test_archive::GEOMETRY;
    let options = DecodeOptions::default();
    let baseline = FcstdCodec
        .decode(&mut Cursor::new(source), &options)
        .unwrap();
    let extended = crate::test_support::test_archive::rewrite_entry(
        source,
        "Document.xml",
        |data| {
            let document = std::str::from_utf8(data).unwrap();
            let marker = "<Object name=\"CorpusMetadata\">";
            assert_eq!(document.matches(marker).count(), 1);
            document
            .replacen(
                marker,
                &format!("{marker}<Extensions Count=\"1\"><Extension name=\"Independent\" type=\"Vendor::Independent\"/></Extensions>"),
                1,
            )
            .into_bytes()
        },
    );
    for (prefix, canonical) in [
        ("<ObjectDeps Name=\"CorpusMetadata\"", "0"),
        ("<Extensions", "1"),
    ] {
        for declared in [
            None,
            Some("99"),
            Some("bad"),
            Some("999999999999999999999999999999"),
        ] {
            let bytes = crate::test_support::test_archive::rewrite_entry(
                &extended,
                "Document.xml",
                |data| {
                    let document = std::str::from_utf8(data).unwrap();
                    let old = format!("{prefix} Count=\"{canonical}\"");
                    let changed = match declared {
                        Some(value) => format!("{prefix} Count=\"{value}\""),
                        None => prefix.to_owned(),
                    };
                    assert_eq!(document.matches(&old).count(), 1);
                    document.replacen(&old, &changed, 1).into_bytes()
                },
            );
            let decoded = FcstdCodec
                .decode(&mut Cursor::new(bytes), &options)
                .unwrap();
            assert_eq!(decoded.ir().model, baseline.ir().model);
            crate::test_support::test_archive::assert_valid_document(decoded.ir());
            assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
            assert!(decoded.report().losses.iter().any(|loss| loss.code
                == crate::loss::FreecadLossCode::PersistenceCountNoncanonical
                    .note(String::new())
                    .code));
        }
    }
}

#[test]
fn source_only_properties_repack_without_inventing_values_or_attachments() {
    for payload in [
        r#"<Property name="Unused" type="App::PropertyLink"><String value="wrong payload"/></Property>"#,
        r#"<Property name="Unused" type="App::PropertyFile"><File name="missing-notes.txt"/></Property>"#,
    ] {
        let xml = format!(
            r#"<Document SchemaVersion="4" FileVersion="1"><Properties Count="1">{payload}</Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#
        );
        let bytes = crate::test_support::test_archive::archive(&xml);
        let decoded = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let mut written = Vec::new();
        FcstdCodec
            .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut written))
            .unwrap();
        let roundtrip = FcstdCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap();
        assert_eq!(roundtrip.ir().model, decoded.ir().model);
        let properties = roundtrip
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<PropertyRecord>("properties")
            .unwrap();
        assert_eq!(properties.len(), 1);
        assert_eq!(properties[0].xml.text(), payload);
        assert!(properties[0].links().is_empty());
        assert!(properties[0].side_entries().is_empty());
        assert!(crate::validate_native(
            &cadmpeg_test_support::service_decode_context(),
            roundtrip.ir()
        )
        .unwrap()
        .is_empty());
    }
}
