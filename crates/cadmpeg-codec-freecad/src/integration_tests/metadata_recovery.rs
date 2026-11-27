// SPDX-License-Identifier: Apache-2.0
//! Independent persistence metadata, source retention, and complete codec checks.

use crate::native::PropertyRecord;
use crate::test_support::test_archive::{archive, archive_entries};
use crate::FcstdCodec;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
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
    assert_eq!(decoded.ir().geometry, baseline.ir().geometry);
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
    assert!(!baseline.ir().geometry.faces.is_empty());
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(&changed), &DecodeOptions::default())
        .unwrap();
    assert_eq!(decoded.ir().geometry, baseline.ir().geometry);
    assert_eq!(decoded.ir().model, baseline.ir().model);
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
    crate::test_support::test_archive::assert_valid_document(decoded.ir());
    let physical = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
        .unwrap();
    let [(_, retained)] = given_up_entries(decoded).try_into().unwrap();
    let start = usize::try_from(retained.offset()).unwrap();
    let end = usize::try_from(retained.end_offset()).unwrap();
    assert!(physical.last().unwrap().span.end() < retained.end_offset());
    assert_eq!(retained.data(), Some(&changed[start..end]));
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
        &cadmpeg_core::decode::test_support::service_decode_context(),
        decoded.ir()
    )
    .expect("native validation")
    .is_empty());
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("neutral validation")
        .is_ok());
    let admitted = decoded
        .ir()
        .native
        .namespace("fcstd")
        .expect("native graph")
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("admitted entries");
    assert!(!admitted.iter().any(|entry| entry.name() == "Thumbnail.png"));
    let losses: Vec<_> = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/record.unreadable")
        .map(|loss| loss.message.clone())
        .collect();
    let [(stub, retained)] = given_up_entries(decoded).try_into().unwrap();
    let [message] = losses.as_slice() else {
        panic!("one record.unreadable loss: {losses:?}");
    };
    assert!(message.starts_with("part archive-entry \"Thumbnail.png\""));
    assert!(message.contains(&format!("record={}", stub.id)));
    assert!(!retained.data().expect("inline bytes").is_empty());
}

#[test]
fn an_empty_unreadable_payload_is_given_up_with_no_bytes() {
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
    let [(stub, retained)] = given_up_entries(result).try_into().unwrap();
    assert_eq!(stub.links[0].as_str(), "fcstd:native:document#0");
    assert_eq!(retained.data(), Some([].as_slice()));
}

/// The stub and retained bytes of each archive entry the decode gave up.
fn given_up_entries(
    result: cadmpeg_ir::DecodeResult,
) -> Vec<(
    cadmpeg_ir::NativeUnknownRecord,
    cadmpeg_ir::source_fidelity::RetainedSourceRecord,
)> {
    let (ir, _, fidelity) = result.into_parts();
    ir.native_unknowns("fcstd")
        .unwrap()
        .into_iter()
        .filter(|stub| stub.id.as_str().starts_with("fcstd:source:part#"))
        .map(|stub| {
            let retained = fidelity
                .retained_record(stub.id.as_str())
                .expect("the given-up entry's bytes are retained")
                .clone();
            (stub, retained)
        })
        .collect()
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
                &cadmpeg_core::decode::test_support::service_decode_context(),
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
        for index in 0..256 {
            writeln!(addition, r#"<Property name="Stress{index}" type="App::PropertyString"><String value=""/></Property>"#).unwrap();
        }
        document
            .replacen("<Properties Count=\"17\"", "<Properties Count=\"273\"", 1)
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
    assert_eq!(result.ir().geometry, baseline.ir().geometry);
    assert_eq!(result.ir().model, baseline.ir().model);
    crate::test_support::test_archive::assert_valid_document(result.ir());
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &options.policy).unwrap();
    assert!(crate::validate_native(&ctx, result.ir())
        .unwrap()
        .is_empty());
    let properties = result.ir().native.namespace("fcstd").unwrap().arenas();
    assert!(properties["properties"].len() >= 256);
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
            assert_eq!(decoded.ir().geometry, baseline.ir().geometry);
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
            .plan(EncodeInput::new(decoded.ir(), None))
            .and_then(|plan| plan.write_to(&mut written))
            .unwrap();
        let roundtrip = FcstdCodec
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .unwrap();
        assert_eq!(roundtrip.ir().geometry, decoded.ir().geometry);
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
            &cadmpeg_core::decode::test_support::service_decode_context(),
            roundtrip.ir()
        )
        .unwrap()
        .is_empty());
    }
}
