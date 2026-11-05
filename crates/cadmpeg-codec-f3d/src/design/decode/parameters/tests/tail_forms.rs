// SPDX-License-Identifier: Apache-2.0
//! Parameter-tail and header-form tests.

use super::*;

fn parameter_text_field(text: &str) -> Vec<u8> {
    let mut field = Vec::new();
    lp_utf16(&mut field, text);
    field
}

/// The parameter tail is a reference to the stream's parameter collection, so
/// every header form admits any collection record index.
#[test]
fn parameter_tail_names_any_collection_record() {
    let set_target = |frame: &mut Vec<u8>, target: u32| {
        let tail = frame.len() - 10;
        frame[tail..tail + 4].copy_from_slice(&target.to_le_bytes());
    };
    let compact = |target| {
        let mut frame = compact_owned_parameter_record(
            428,
            9,
            "3.40 mm",
            "HoleDiameter",
            Some("mm"),
            "d9",
            0.34,
        );
        set_target(&mut frame, target);
        frame
    };
    let mut discriminated =
        parameter_record(Some(428), "3.40 mm", "HoleDiameter", Some("mm"), "d9", 0.34);
    set_target(&mut discriminated, 175);
    assert_eq!(
        parse_design_parameter_record(&discriminated)
            .expect("discriminated parameter")
            .name(),
        "d9"
    );
    let mut class_287 = class_287_parameter_record("HoleDepth", "d20");
    set_target(&mut class_287, 174);
    assert_eq!(
        parse_design_parameter_record(&class_287)
            .expect("class-287 parameter")
            .name(),
        "d20"
    );
    for target in [18, 19, 173, 174, 175, 0x0001_00AF] {
        assert_eq!(
            parse_design_parameter_record(&compact(target))
                .expect("parameter naming a collection")
                .name(),
            "d9"
        );
    }

    // A marked reference names a record, so its index is not zero.
    assert!(parse_design_parameter_record(&compact(0)).is_none());
    // The reference occupies eleven bytes: six zero bytes follow its index.
    let mut trailing = compact(175);
    *trailing.last_mut().unwrap() = 1;
    assert!(parse_design_parameter_record(&trailing).is_none());
    // Twelve bytes end the frame.
    let mut longer = compact(175);
    longer.push(0);
    assert!(parse_design_parameter_record(&longer).is_none());
}

/// A family discriminator does not depend on the source kind or the tail.
#[test]
fn parameter_discriminator_is_independent_of_source_kind_and_tail() {
    for discriminator in [0u64, 1, 2, 3, 4, 5, 6] {
        for target in [16u32, 19, 175] {
            for (source_kind, name) in [("AlongDistance", "d12"), ("TangencyWeight", "d81")] {
                let mut frame =
                    parameter_record(Some(44), "Width / 2", source_kind, Some("mm"), name, 3.0);
                frame[22..30].copy_from_slice(&discriminator.to_le_bytes());
                let tail = frame.len() - 10;
                frame[tail..tail + 4].copy_from_slice(&target.to_le_bytes());
                assert_eq!(
                    parse_design_parameter_record(&frame)
                        .expect("discriminated parameter")
                        .family_discriminator()
                        .map(|value| value.value.code()),
                    Some(discriminator)
                );
            }
        }
    }
    let mut frame = parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    );
    frame[22..30].copy_from_slice(&7u64.to_le_bytes());
    assert!(parse_design_parameter_record(&frame).is_none());
}

/// Discriminator 0 leaves offsets 22 through 29 zero, so the narrow reading is
/// tried first and the wide reading follows when the narrow reading does not close.
#[test]
fn wide_parameter_with_discriminator_0_reads_after_the_narrow_form_fails() {
    for (source_ordinal, owner) in [(4u32, 229u32), (1, 201), (0x200, 0x2_C2E2), (0x100, 0x100)] {
        let mut frame = parameter_record(Some(owner), "t", "AlongDistance", Some("mm"), "d4", 0.6);
        frame[22..30].copy_from_slice(&0u64.to_le_bytes());
        frame[31..35].copy_from_slice(&source_ordinal.to_le_bytes());
        frame.splice(22..22, [0; 4]);
        let expression_end = 50 + 4 + 2 * "t".len();
        frame.splice(expression_end..expression_end, [0; 2]);
        let parameter = parse_design_parameter_record(&frame).expect("wide parameter");
        assert_eq!(parameter.source_ordinal, source_ordinal);
        assert_eq!(parameter.owner_record_index(), Some(owner));
        assert_eq!(parameter.expression(), "t");
        assert_eq!(parameter.source_kind(), "AlongDistance");
        let discriminator = parameter.family_discriminator().expect("discriminator");
        assert_eq!(
            (discriminator.value.code(), discriminator.offset),
            (0, Some(26))
        );
    }
    let mut document = parameter_record(None, "3 mm", "User Parameter", Some("mm"), "w", 0.3);
    document[22..30].copy_from_slice(&0u64.to_le_bytes());
    document[31..35].copy_from_slice(&3u32.to_le_bytes());
    document.splice(22..22, [0; 4]);
    let expression_end = 40 + 4 + 2 * "3 mm".len();
    document.splice(expression_end..expression_end, [0; 2]);
    let parameter = parse_design_parameter_record(&document).expect("wide document parameter");
    assert_eq!(parameter.source_ordinal, 3);
    assert_eq!(parameter.owner_record_index(), None);
    assert_eq!(parameter.source_kind(), "User Parameter");

    // The eleven-byte document trailer marks exact source kind `User Parameter`.
    let kind_at = 40 + 4 + 2 * "3 mm".len() + 11;
    let mut localized = document.clone();
    localized.splice(
        kind_at..kind_at + 4 + 2 * "User Parameter".len(),
        parameter_text_field("Paramètre utilisateur"),
    );
    assert!(parse_design_parameter_record(&localized).is_none());

    // The eleven-byte document trailer belongs to discriminator 0.
    let mut other = document;
    other[26..34].copy_from_slice(&6u64.to_le_bytes());
    assert!(parse_design_parameter_record(&other).is_none());

    // The narrow reading wins when it closes.
    let mut narrow = parameter_record(Some(229), "t", "AlongDistance", Some("mm"), "d4", 0.6);
    narrow[22..30].copy_from_slice(&0u64.to_le_bytes());
    let parameter = parse_design_parameter_record(&narrow).expect("narrow parameter");
    assert_eq!(
        parameter.family_discriminator().map(|field| field.offset),
        Some(Some(22))
    );
}

#[test]
fn decode_reports_frames_that_name_a_parameter_collection_and_match_no_grammar() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = parameter_record(Some(44), "first", "AlongDistance", Some("mm"), "d1", 1.0);
    bulk[4..7].copy_from_slice(b"256");
    let mut records = vec![(71, 0)];
    let mut rejected = parameter_record(Some(46), "second", "AlongDistance", Some("mm"), "d2", 2.0);
    rejected[4..7].copy_from_slice(b"256");
    rejected[7] = 47;
    let expression_end = 46 + 4 + 2 * "second".len();
    rejected[expression_end] = 7;
    records.push((47, u64::try_from(bulk.len()).unwrap()));
    bulk.extend_from_slice(&rejected);
    let mut other_target = rejected.clone();
    other_target[7] = 49;
    let tail = other_target.len() - 10;
    other_target[tail] = 20;
    records.push((49, u64::try_from(bulk.len()).unwrap()));
    bulk.extend_from_slice(&other_target);
    // The document flag with another source kind matches no grammar.
    let mut localized =
        parameter_record(None, "12 mm", "Paramètre utilisateur", Some("mm"), "o", 1.2);
    localized[4..7].copy_from_slice(b"256");
    localized[7] = 51;
    records.push((51, u64::try_from(bulk.len()).unwrap()));
    bulk.extend_from_slice(&localized);
    // A non-finite value closes the grammar and defines no record; the decode
    // continues.
    let mut non_finite = parameter_record(
        Some(54),
        "12 mm",
        "AlongDistance",
        Some("mm"),
        "n",
        f64::NAN,
    );
    non_finite[4..7].copy_from_slice(b"256");
    non_finite[7] = 55;
    records.push((55, u64::try_from(bulk.len()).unwrap()));
    bulk.extend_from_slice(&non_finite);
    let mut last = parameter_record(Some(52), "last", "AlongDistance", Some("mm"), "d3", 3.0);
    last[4..7].copy_from_slice(b"256");
    last[7] = 53;
    records.push((53, u64::try_from(bulk.len()).unwrap()));
    bulk.extend_from_slice(&last);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (
                "00000000-0000-0000-0000-000000000001",
                "",
                1,
                "Fusion",
                &[71, 47, 49, 51, 55, 53],
            ),
            (
                "B0556F58-EE09-46A7-B18F-E296F48E5B13",
                "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
                3,
                "CommonData",
                &[19],
            ),
        ],
        &records,
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = decode_archive(&archive).unwrap();
    let names: Vec<_> = decoded
        .parameters
        .iter()
        .map(crate::records::parameters::DesignParameter::name)
        .collect();
    assert_eq!(names, ["d1", "d3"]);
    assert_eq!(decoded.losses.len(), 3);
    assert!(decoded
        .losses
        .iter()
        .all(|loss| loss.code.local_code() == "record.unreadable" && loss.provenance.is_some()));
}

#[test]
fn owned_discriminated_parameter_retains_six_expression_flags() {
    let mut frame = parameter_record(Some(44), "1", "TangencyWeight", None, "d287", 1.0);
    let expression_end = 46 + 4 + 2;
    frame.splice(
        expression_end..expression_end + 9,
        [0, 0, 0, 0, 0, 1, 0, 0, 0, 0],
    );
    let parameter = parse_design_parameter_record(&frame).expect("extended flag form");
    assert_eq!(parameter.owner_record_index(), Some(44));
    assert_eq!(parameter.expression(), "1");
    assert_eq!(parameter.source_kind(), "TangencyWeight");
    assert_eq!(parameter.name(), "d287");
    assert_eq!(parameter.evaluated_value().get(), 1.0);
    let value = serde_json::to_value(&parameter).unwrap();
    assert_eq!(
        value["expression_flags"],
        serde_json::json!([false, true, false, false, false, false])
    );
    let roundtrip: crate::records::parameters::DesignParameter =
        serde_json::from_value(value).unwrap();
    assert_eq!(
        serde_json::to_value(roundtrip).unwrap()["expression_flags"],
        serde_json::json!([false, true, false, false, false, false])
    );
}
