// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn scan_decodes_active_principal_unit() {
    let mut payload = visibgeom_payload(5, 12);
    payload.extend_from_slice(b"_principal_sys_units_id\0\x33");
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let scan = container::scan_bytes_ok(data);

    assert_eq!(
        scan.framing
            .principal_unit
            .map(|unit| unit.to_string())
            .as_deref(),
        Some("mmNs")
    );
}

#[test]
fn legacy_principal_unit_sets_the_source_length_scale() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @root 28 0\n1 28 ->\n\
        @principal_sys_units 25 10\n2 25 Inch lbm Second (Pro/E Default)\n\
        @rel_accuracy 26 2\n2 26 3FF\n\
        @feat_id 27 1\n2 27 42\n\
        @encoded 29 10\n2 29 \xE9\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("legacy unit decode");
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["principal_unit"], "inLbmS");
    assert_eq!(source.attributes["source_length_scale_mm"], "25.4");
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_principal_unit_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_real_scalar_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_real_element_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_integer_scalar_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_object_arrow_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_string_scalar_count"],
        2
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_string_element_count"],
        2
    );
    assert_eq!(
        wire::coverage(result.report())["undecoded_legacy_string_encoding_count"],
        1
    );
    let reals = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace")
        .arenas()["legacy_real_values"];
    assert_eq!(reals.len(), 1);
    assert_eq!(
        reals[0].field("name"),
        Some(serde_json::json!("rel_accuracy"))
    );
    assert_eq!(
        reals[0].field("payload"),
        Some(serde_json::json!({"form": "scalar", "value": 1.0}))
    );
    let integers = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace")
        .arenas()["legacy_integer_values"];
    assert_eq!(integers.len(), 1);
    assert_eq!(
        integers[0].field("name"),
        Some(serde_json::json!("feat_id"))
    );
    assert_eq!(
        integers[0].field("payload"),
        Some(serde_json::json!({"form": "scalar", "value": 42}))
    );
    let objects = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace")
        .arenas()["legacy_objects"];
    assert_eq!(objects.len(), 1);
    assert_eq!(
        integers[0].field("parent"),
        Some(serde_json::json!(objects[0].id()))
    );
    let strings = &result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace")
        .arenas()["legacy_string_values"];
    assert_eq!(strings.len(), 2);
    let principal = strings
        .iter()
        .find(|record| record.field("name") == Some(serde_json::json!("principal_sys_units")))
        .expect("principal-unit string");
    let encoded = strings
        .iter()
        .find(|record| record.field("name") == Some(serde_json::json!("encoded")))
        .expect("encoded string");
    assert_eq!(
        principal.field("payload"),
        Some(serde_json::json!({
            "form": "scalar",
            "value": {
                "form": "utf8",
                "text": "Inch lbm Second (Pro/E Default)"
            }
        }))
    );
    assert_eq!(
        encoded.field("payload"),
        Some(serde_json::json!({
            "form": "scalar",
            "value": {"form": "bytes", "bytes": [233]}
        }))
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == CreoLossCode::LegacyStringEncodingRetained.kind())
            .count(),
        1
    );
}

#[test]
fn legacy_numbered_numeric_families_emit_exact_native_values() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @root 1 0\n@five 2 5\n@five_array 3 5\n@six 4 6\n@six_array 5 6\n\
        @seven 6 7\n@nine 7 9\n@eleven 8 11\n0 1 ->\n1 2 2700\n\
        1 3 [3]\n$0,2*144\n1 4 400\n1 5 [2][2]\n$3FF,3*0\n1 6 7\n\
        1 7 [4]\n$0,67108864,2*1\n1 8 [1]\n2 8 14633\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("legacy numbered numeric decode");
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_5_scalar_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_5_array_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_5_element_count"],
        4
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_6_scalar_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_6_array_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_6_element_count"],
        5
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_7_scalar_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_9_array_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_11_array_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_11_element_count"],
        1
    );

    let native = result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace");
    assert_eq!(native.arenas()["legacy_type_5_values"].len(), 2);
    assert_eq!(native.arenas()["legacy_type_6_values"].len(), 2);
    assert_eq!(native.arenas()["legacy_type_7_values"].len(), 1);
    assert_eq!(native.arenas()["legacy_type_9_values"].len(), 1);
    assert_eq!(native.arenas()["legacy_type_11_values"].len(), 1);
    assert_eq!(
        native.arenas()["legacy_type_6_values"]
            .iter()
            .find(|record| record.field("name") == Some(serde_json::json!("six")))
            .and_then(|record| record.field("payload")),
        Some(serde_json::json!({"form": "scalar", "value": 2.0}))
    );
    assert_eq!(
        native.arenas()["legacy_type_11_values"][0].field("payload"),
        Some(serde_json::json!({
            "form": "array",
            "dimensions": [1],
            "runs": [{"count": 1, "value": 14633}]
        }))
    );
    assert_eq!(
        native.arenas()["legacy_type_11_values"][0].field("parent"),
        Some(serde_json::json!(native.arenas()["legacy_objects"][0].id()))
    );
}

#[test]
fn legacy_type_3_and_type_4_emit_exact_scalar_bytes() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @root 1 0\n@three_null 2 3\n@three_text 3 3\n@four 4 4\n0 1 ->\n\
        1 2 NULL\n1 3 texture-name\n1 4 NULL\n#END_OF_P_OBJECT\n\
        #Pro/ENGINEER  TM  Version H-01-21\n";

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("legacy type-3/type-4 decode");
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_3_scalar_count"],
        2
    );
    assert_eq!(
        wire::coverage(result.report())["decoded_legacy_type_4_scalar_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["unresolved_legacy_type_3_value_count"],
        0
    );
    assert_eq!(
        wire::coverage(result.report())["unresolved_legacy_type_4_value_count"],
        0
    );

    let native = result
        .ir()
        .native
        .namespace("creo")
        .expect("Creo namespace");
    assert_eq!(native.arenas()["legacy_type_3_values"].len(), 2);
    assert_eq!(native.arenas()["legacy_type_4_values"].len(), 1);
    assert_eq!(
        native.arenas()["legacy_type_3_values"][0].field("payload"),
        Some(serde_json::json!({"form": "null"}))
    );
    assert_eq!(
        native.arenas()["legacy_type_3_values"][1].field("payload"),
        Some(serde_json::json!({"form": "utf8", "text": "texture-name"}))
    );
    assert_eq!(
        native.arenas()["legacy_type_4_values"][0].field("payload"),
        Some(serde_json::json!({"form": "utf8", "text": "NULL"}))
    );
    assert_eq!(
        native.arenas()["legacy_type_4_values"][0].field("parent"),
        Some(serde_json::json!(native.arenas()["legacy_objects"][0].id()))
    );
}

#[test]
fn incomplete_legacy_values_are_reported() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        @values 7 2\n0 7 [2]\n$0\n\
        @ids 8 1\n0 8 [2]\n$1\n\
        @objects 9 0\n0 9 [2]\n\
        @future 10 0\n0 10 token\n\
        @names 11 10\n0 11 [2]\n1 11 only\n\
        @continued 12 10\n0 12 first\n$second\n\
        @type_three_continued 13 3\n0 13 first\n$second\n\
        @type_four_bytes 14 4\n0 14 \xff\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("legacy incomplete-array decode");
    assert_eq!(
        wire::coverage(result.report())["unresolved_legacy_real_value_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["unresolved_legacy_integer_value_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["incomplete_legacy_object_array_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["unresolved_legacy_object_value_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["incomplete_legacy_string_array_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["unresolved_legacy_string_value_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["unresolved_legacy_type_3_value_count"],
        1
    );
    assert_eq!(
        wire::coverage(result.report())["undecoded_legacy_type_4_encoding_count"],
        1
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| {
                loss.code.taxonomy() == cadmpeg_ir::report::loss::LossTaxonomy::RecordNotTyped
            })
            .count(),
        7
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == CreoLossCode::LegacyByteStringEncodingRetained.kind())
            .count(),
        1
    );
}

#[test]
fn complete_header_adjacent_p_object_selects_legacy_ascii_layout() {
    let data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n\
        #P_OBJECT 6\n@P_object 1 0\n0 1 ->\n@value #END_OF_P_OBJECT\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Version H-01-21\n";
    let scan = container::scan_bytes_ok(data);

    assert!(matches!(scan.framing.layout, Layout::LegacyAscii(_)));
    assert_eq!(scan.framing.layout.token(), "LEGACY_ASCII");
    assert!(scan.framing.sections.is_empty());
    let legacy = scan.framing.layout.legacy_ascii().expect("legacy framing");
    assert_eq!(legacy.schema, "6");
    assert_eq!(legacy.product_release.as_deref(), Some("H-01-21"));
    assert_eq!(legacy.persistence.declaration_count(), 1);
    assert_eq!(legacy.persistence.value_count(), 1);
    let classification =
        crate::decode::with_test_decode_ctx(|ctx| crate::dialect::classify(ctx, &scan))
            .expect("dialect classification admitted");
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| container::summarize(ctx, &scan, classification))
            .expect("container summary admitted")
            .notes
            .iter()
            .any(|note| {
                note.contains("legacy ASCII persistence: schema 6; product release H-01-21")
            })
    );
}

#[test]
fn legacy_ascii_toc_is_authoritative_for_named_section_extents() {
    let mut data = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        #END_OF_P_OBJECT\n"
        .to_vec();
    let banner_offset = data.len();
    data.extend_from_slice(
        b"#Pro/ENGINEER  TM  Version H-01-21\n@Toc 52 0\n0 52 ->\n\
          @entry 53 10\n1 53 [2]\n",
    );
    let section = b"#BasicData\n@field 1 1\n0 1 4\n#FakeSection\nembedded";
    let row_tail = format!(" {:08x} 0 983####\n2 53 ####\n", section.len());
    let relative_offset =
        data.len() + b"2 53 BasicData ".len() + 8 + row_tail.len() - banner_offset;
    data.extend_from_slice(format!("2 53 BasicData {relative_offset:08x}{row_tail}").as_bytes());
    let section_offset = data.len();
    data.extend_from_slice(section);

    let scan = container::scan_bytes_ok(data);

    assert!(matches!(scan.framing.layout, Layout::LegacyAscii(_)));
    assert_eq!(scan.framing.sections.len(), 1);
    assert_eq!(scan.framing.sections[0].name(), "BasicData");
    assert_eq!(scan.framing.sections[0].offset(), section_offset);
    assert_eq!(scan.framing.sections[0].length(), section.len());
    let persistence = &scan
        .framing
        .layout
        .legacy_ascii()
        .expect("legacy framing")
        .persistence;
    assert_eq!(persistence.scopes.len(), 2);
    assert_eq!(persistence.declaration_count(), 3);
    assert_eq!(persistence.value_count(), 5);
}

#[test]
fn legacy_release_banner_and_unspecified_banner_preserve_framing_metadata() {
    let release = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 12\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Release 16.0  All Rights Reserved\n";
    let scan = container::scan_bytes_ok(release.as_slice());
    let legacy = scan.framing.layout.legacy_ascii().expect("legacy framing");
    assert_eq!(legacy.schema, "12");
    assert_eq!(legacy.product_release.as_deref(), Some("16.0"));

    let result = CreoCodec
        .decode(&mut Cursor::new(release), &DecodeOptions::default())
        .expect("legacy container decode");
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.attributes["legacy_ascii_schema"], "12");
    assert_eq!(source.attributes["legacy_ascii_product_release"], "16.0");
    assert_eq!(source.attributes["legacy_ascii_declaration_count"], "0");
    assert_eq!(source.attributes["legacy_ascii_scope_count"], "1");
    assert_eq!(source.attributes["legacy_ascii_value_count"], "0");
    assert_eq!(
        source.attributes["legacy_ascii_conflicting_declaration_count"],
        "0"
    );

    let concatenated_release = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER  TM  Release18.0  All Rights Reserved\n";
    let scan = container::scan_bytes_ok(concatenated_release.as_slice());
    let legacy = scan.framing.layout.legacy_ascii().expect("legacy framing");
    assert_eq!(legacy.product_release.as_deref(), Some("18.0"));

    let unspecified = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER\n";
    let scan = container::scan_bytes_ok(unspecified.as_slice());
    let legacy = scan.framing.layout.legacy_ascii().expect("legacy framing");
    assert_eq!(legacy.product_release, None);
}

#[test]
fn incomplete_or_payload_embedded_p_object_does_not_select_legacy_ascii_layout() {
    let incomplete = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT 6\n@P_object 1 0\n".to_vec();
    assert_eq!(
        container::scan_bytes_ok(incomplete).framing.layout,
        Layout::Unknown(UnknownLayout::NoDiscriminant)
    );
    let empty_schema = b"#UGC:2 PART 1\n#-END_OF_UGC_HEADER\n#P_OBJECT \n\
        #END_OF_P_OBJECT\n#Pro/ENGINEER";
    assert_eq!(
        container::scan_bytes_ok(empty_schema).framing.layout,
        Layout::Unknown(UnknownLayout::NoDiscriminant)
    );

    let embedded = build_prt_raw(
        "c",
        &[(
            "VisibGeom",
            b"#P_OBJECT 6\n#END_OF_P_OBJECT\n#Pro/ENGINEER".to_vec(),
        )],
    );
    assert_eq!(
        container::scan_bytes_ok(embedded).framing.layout,
        Layout::Unknown(UnknownLayout::NoDiscriminant)
    );
}

