// SPDX-License-Identifier: Apache-2.0

#[test]
fn fixture_attributed_frames_retain_reference_namespaces_and_independent_ordinals() {
    #[derive(serde::Deserialize)]
    struct Case {
        label: String,
        source_hex: cadmpeg_ir::native::bytes::NativeBytes,
        component: u64,
        ordinal: u32,
        path_discriminator: u32,
        component_guid: String,
        occurrence_key: String,
        key_record_index: u64,
        key_segment_index: u32,
        owner: u64,
        matrix: Option<[f64; 16]>,
    }
    let cases: Vec<Case> = serde_json::from_str(&crate::test_support::verification::text(
        "xref/attributed_records.json",
        include_str!("../../../../test_support/xref/attributed_records.json"),
    ))
    .unwrap();
    for case in cases {
        let occurrence = super::super::attributed_component_occurrence(
            case.source_hex.as_ref(),
            0,
            "f3d:Design/BulkStream.dat",
        )
        .unwrap_or_else(|| panic!("{}", case.label));
        assert_eq!(
            occurrence.component_record_index, case.component,
            "{}",
            case.label
        );
        assert_eq!(
            occurrence.occurrence_ordinal(),
            case.ordinal,
            "{}",
            case.label
        );
        assert_eq!(
            occurrence.component_guid.as_str(),
            case.component_guid,
            "{}",
            case.label
        );
        assert_eq!(
            occurrence.occurrence_guid.as_str(),
            case.occurrence_key,
            "{}",
            case.label
        );
        assert_eq!(occurrence.owner_entity, case.owner, "{}", case.label);
        assert_eq!(
            occurrence
                .transform()
                .map(|matrix| matrix.value.rows().concat()),
            case.matrix.map(|matrix| matrix.to_vec()),
            "{}",
            case.label
        );
        let wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(
            wire["attributed"]["path_discriminator"], case.path_discriminator,
            "{}",
            case.label
        );
        assert_eq!(
            wire["attributed"]["key_record_index"], case.key_record_index,
            "{}",
            case.label
        );
        assert_eq!(
            wire["attributed"]["key_segment_index"], case.key_segment_index,
            "{}",
            case.label
        );
        let decoded: crate::records::feature::assembly_features::DesignComponentOccurrence =
            serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, occurrence, "{}", case.label);
    }
}

#[test]
fn empty_attributed_occurrence_keeps_its_prefix_key_namespace_and_matrix() {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"731", 800);
    bytes.resize(378, 0);
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..33].copy_from_slice(&305u64.to_le_bytes());
    bytes[35..39].copy_from_slice(&7u32.to_le_bytes());
    bytes[39] = 1;
    bytes[44..46].copy_from_slice(&2u16.to_le_bytes());
    bytes[46..50].copy_from_slice(&3u32.to_le_bytes());
    super::guid(&mut bytes, 50, "11111111-2222-4333-8444-555555555555");
    super::guid(&mut bytes, 126, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee");
    bytes[202] = 1;
    bytes[203..211].copy_from_slice(&24u64.to_le_bytes());
    bytes[212] = 1;
    bytes[213..217].copy_from_slice(&9u32.to_le_bytes());
    bytes[218] = 1;
    bytes[219..227].copy_from_slice(&305u64.to_le_bytes());
    let matrix: [f64; 16] = [
        1.0, 0.0, 0.0, 2.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    for (ordinal, value) in matrix.into_iter().enumerate() {
        bytes[230 + ordinal * 8..238 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[367] = 1;
    bytes[368..376].copy_from_slice(&12u64.to_le_bytes());
    crate::test_support::indexed_header(&mut bytes, *b"732", 801);
    let occurrence =
        super::super::attributed_component_occurrence(&bytes, 0, "f3d:Design/BulkStream.dat")
            .unwrap();
    assert_eq!(occurrence.component_record_index, 305);
    assert_eq!(occurrence.occurrence_ordinal(), 3);
    assert_eq!(occurrence.owner_entity, 12);
    assert_eq!(
        occurrence.transform().unwrap().value.rows().concat(),
        matrix
    );
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert_eq!(wire["attributed"]["attributes"], serde_json::json!([]));
    assert_eq!(wire["attributed"]["ordinal_prefix"], 2);
    assert_eq!(wire["attributed"]["path_discriminator"], 7);
    assert_eq!(wire["attributed"]["key_record_index"], 24);
    assert_eq!(wire["attributed"]["key_segment_index"], 9);
    assert!(wire["attributed"]["source_record_index"].is_null());
    assert_eq!(wire["component_guid_offset"], 54);
    assert_eq!(wire["occurrence_guid_offset"], 130);
    assert_eq!(wire["transform_offset"], 230);
    assert_eq!(wire["owner_entity_offset"], 368);
    let decoded: crate::records::feature::assembly_features::DesignComponentOccurrence =
        serde_json::from_value(wire).unwrap();
    assert_eq!(decoded, occurrence);
}
