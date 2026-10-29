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
    drop(snapshot);
    drop(ctx);
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
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [(_, retained)] = parts.as_slice() else {
        panic!("one given-up part: {parts:?}")
    };
    assert!(!namespace.arenas().contains_key("unreadable_entries"));
    let start = usize::try_from(retained.offset()).unwrap();
    let end = start + usize::try_from(retained.byte_len()).unwrap();
    assert_eq!(retained.data(), Some(&changed[start..end]));
    let archive_arena = DecodeArena::new();
    let (archive_ctx, archive_root) =
        DecodeContext::from_root_bytes(&changed, &archive_arena, &policy).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&archive_ctx, archive_root).unwrap();
    let original_span = archive
        .entry("Thumbnail.png")
        .unwrap()
        .stored_range()
        .unwrap();
    let original_start = usize::try_from(original_span.start).unwrap();
    let original_end = usize::try_from(original_span.end).unwrap();
    assert_eq!(
        &crate::source_archive::image(decoded.source_fidelity()).unwrap()
            [original_start..original_end],
        &changed[original_start..original_end]
    );
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
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [(id, retained)] = parts.as_slice() else {
        panic!("one given-up part: {parts:?}")
    };
    assert_eq!(usize::try_from(retained.offset()).unwrap(), payload);
    assert_eq!(
        retained.data(),
        Some(&changed[payload..payload + b"preview".len()])
    );
    assert!(!crate::test_support::source_fidelity::bytes(decoded.source_fidelity(), id).is_empty());
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
    drop(snapshot);
    drop(ctx);
    bytes[local + 8..local + 10].copy_from_slice(&12_u16.to_le_bytes());
    bytes[central + 10..central + 12].copy_from_slice(&12_u16.to_le_bytes());
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    let parts = crate::test_support::source_fidelity::given_up(result.source_fidelity());
    let [(_, retained)] = parts.as_slice() else {
        panic!("one given-up part: {parts:?}")
    };
    assert_eq!(retained.byte_len(), 0);
    assert_eq!(retained.data(), Some([].as_slice()));
    assert!(crate::source_archive::image(result.source_fidelity()).is_ok());
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
    // Rule P gives every readable extra string a neutral attribute. Preserve
    // the complete original model and add exactly the authored Stress values.
    let mut expected = baseline.ir().model.clone();
    let owner = crate::native::native_id("document", "0");
    for index in 0..32_000 {
        let name = format!("Stress{index}");
        let property = crate::native::native_child_id("property", &owner, &name);
        expected
            .attributes
            .push(cadmpeg_ir::attributes::SourceAttribute {
                native_ref: Some(property.clone()),
                id: cadmpeg_ir::ids::AttributeId::mint(crate::native::model_id(
                    "attribute",
                    &property,
                    "property",
                ))
                .unwrap(),
                target: cadmpeg_ir::attributes::AttributeTarget::Document,
                name,
                values: vec![Some(cadmpeg_ir::attributes::AttributeValue::String(
                    String::new(),
                ))],
            });
    }
    expected
        .attributes
        .sort_by(|left, right| left.id.cmp(&right.id));
    assert_eq!(result.ir().model, expected);
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
fn source_only_properties_synthesize_with_explicit_omissions() {
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
        let report = FcstdCodec
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
        assert!(properties.is_empty());
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "metadata.native-property-not-transferred"));
        assert!(matches!(
            report.write_path(),
            cadmpeg_ir::report::export::WritePath::Synthesized { .. }
        ));
        assert!(crate::validate_native(
            &cadmpeg_test_support::service_decode_context(),
            roundtrip.ir()
        )
        .unwrap()
        .is_empty());
    }
}

#[test]
fn a_bodiless_document_property_is_omitted_as_undecoded_source() {
    let xml = r#"<Document SchemaVersion="4" FileVersion="1"><Properties Count="1"><Property name="Bad" type="App::PropertyInteger"><Integer/></Property></Properties><Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(archive(xml)), &DecodeOptions::default())
        .unwrap();
    let property = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<PropertyRecord>("properties")
        .unwrap()
        .into_iter()
        .find(|property| property.name == "Bad")
        .unwrap();
    assert!(property.body.is_none());
    let mut written = Vec::new();
    let report = FcstdCodec
        .plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut written))
        .unwrap();
    // #238, §7.11.2: an omitted property with no decoded body is undecoded
    // source, a Warning, and the loss names the property. The §5.6
    // `native.records-omitted` list names it too.
    let omitted = report
        .losses
        .iter()
        .filter(|loss| {
            loss.message.contains(&property.id)
                && loss.code.local_code() != "native.records-omitted"
        })
        .collect::<Vec<_>>();
    assert_eq!(omitted.len(), 1);
    assert_eq!(
        omitted[0].code,
        crate::loss::FreecadLossCode::ExportNativePropertyNotTransferred
            .note(String::new())
            .code
    );
    assert_eq!(omitted[0].severity, cadmpeg_ir::report::Severity::Warning);
}
