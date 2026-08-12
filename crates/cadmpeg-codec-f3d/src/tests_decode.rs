// SPDX-License-Identifier: Apache-2.0
//! Decode-domain synthetic tests and fixtures.

use super::*;
use cadmpeg_ir::codec::CodecBackend;

pub(super) fn f3d_with_smbh_and_protein(smbh: &[u8]) -> Vec<u8> {
    f3d_with_smbh_and_protein_guids(smbh, &["11111111-2222-3333-4444-555555555555"])
}

pub(super) fn f3d_with_smbh_and_protein_guids(smbh: &[u8], guids: &[&str]) -> Vec<u8> {
    let properties = guids
        .iter()
        .map(|guid| generated_instance_properties_for(guid))
        .collect::<Vec<_>>();
    f3d_with_smbh_and_instance_properties(smbh, &properties)
}

pub(super) fn f3d_with_smbh_and_instance_properties(
    smbh: &[u8],
    properties: &[Vec<u8>],
) -> Vec<u8> {
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let proteins = properties
        .iter()
        .map(|properties| {
            let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
            nested
                .start_file("AssetData/InstanceProperties.bin", stored)
                .unwrap();
            nested.write_all(properties).unwrap();
            nested
                .start_file("AssetData/DefinitionIteratorProperties.bin", stored)
                .unwrap();
            nested
                .write_all(&generated_definition_catalog_for(
                    generated_schema_from_paged(properties),
                ))
                .unwrap();
            nested.finish().unwrap().into_inner()
        })
        .collect::<Vec<_>>();

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
        stored,
    )
    .unwrap();
    zip.write_all(smbh).unwrap();
    for (ordinal, protein) in proteins.iter().enumerate() {
        zip.start_file(
            format!(
                "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"
            ),
            stored,
        )
        .unwrap();
        zip.write_all(protein).unwrap();
    }
    let (design_bulk, design_records) = generated_design_bulkstream();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&design_bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&generated_design_metastream(&design_records))
        .unwrap();
    let (act_bulk, act_records) = generated_act_bulkstream();
    zip.start_file(
        "FusionAssetName[Active]/FusionACTSegmentType1/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(&act_bulk).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/FusionACTSegmentType1/MetaStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(&generated_act_metastream(&act_records))
        .unwrap();
    zip.finish().unwrap().into_inner()
}

/// Build one Design `MetaStream` segment with an empty primary record index.
pub(crate) fn design_metastream(types: &[(&str, &str, u32, &str, &[u64])]) -> Vec<u8> {
    design_metastream_with_records(types, &[])
}

/// Build one Design `MetaStream` segment holding `types`, each entry a
/// `(type GUID, base type GUID, version, module, entity ids)` tuple, and the
/// ordered primary `(entity id, BulkStream offset)` record index. An empty base
/// GUID marks a root type. The segment carries the modern header shape and
/// closes on its own end.
pub(super) fn design_metastream_with_records(
    types: &[(&str, &str, u32, &str, &[u64])],
    records: &[(u64, u64)],
) -> Vec<u8> {
    segment_metastream(
        "Design",
        "FusionDesignSegmentType",
        "Fusion",
        types,
        records,
    )
}

pub(super) fn segment_metastream(
    short_name: &str,
    full_name: &str,
    add_in: &str,
    types: &[(&str, &str, u32, &str, &[u64])],
    records: &[(u64, u64)],
) -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    let mut out = Vec::new();
    lp(&mut out, short_name);
    out.extend_from_slice(&0u32.to_le_bytes());
    let asset_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    out.extend_from_slice(&(asset_guid.encode_utf16().count() as u32).to_le_bytes());
    for unit in asset_guid.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&1234u32.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    lp(&mut out, full_name);
    lp(&mut out, add_in);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(types.len() as u32).to_le_bytes());
    for (type_guid, base_type_guid, version, module, entity_ids) in types {
        lp(&mut out, type_guid);
        lp(&mut out, base_type_guid);
        out.extend_from_slice(&version.to_le_bytes());
        lp(&mut out, module);
        out.extend_from_slice(&(entity_ids.len() as u32).to_le_bytes());
        for entity_id in *entity_ids {
            out.extend_from_slice(&entity_id.to_le_bytes());
        }
    }
    // Empty named-entity list, the primary record index, and an empty secondary
    // index, then the next-entity counter, the flag, and an empty property
    // block.
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for (entity_id, bulk_offset) in records {
        out.extend_from_slice(&entity_id.to_le_bytes());
        out.extend_from_slice(&bulk_offset.to_le_bytes());
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    out
}

pub(super) fn generated_design_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    let base = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    design_metastream_with_records(
        &[
            (
                crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                crate::design::presentation::BODY_PRESENTATION_BASE_TYPE_GUID,
                crate::design::presentation::BODY_PRESENTATION_TYPE_VERSION,
                "Body",
                &[985],
            ),
            (
                crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID,
                base,
                4,
                "MSketch",
                &[277],
            ),
            (
                "33333333-4444-5555-6666-777777777777",
                "",
                5,
                "Dimension",
                &[270, 271],
            ),
            (
                "60403D47-0C49-49B0-BDE8-1679608164A2",
                base,
                1,
                "MSketch",
                &[33, 44],
            ),
            (
                crate::design::presentation::BROWSER_NODE_TYPE_GUID,
                crate::design::presentation::BROWSER_NODE_BASE_TYPE_GUID,
                crate::design::presentation::BROWSER_NODE_TYPE_VERSION,
                crate::records::DESIGN_MODULE_FUSION,
                &[900],
            ),
            (
                crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
                base,
                crate::design::presentation::BREP_CONTAINER_TYPE_VERSION,
                crate::records::DESIGN_MODULE_BODY,
                &[7],
            ),
            (
                crate::design::presentation::BODY_SCENE_NODE_TYPE_GUID,
                base,
                crate::design::presentation::BODY_SCENE_NODE_TYPE_VERSION,
                "Scene",
                &[986],
            ),
            (
                crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                crate::records::DESIGN_MODULE_BODY,
                &[899],
            ),
            (
                "F0130424-8B7E-4092-93C9-1CA807482534",
                base,
                0,
                "Geometry",
                &[600],
            ),
            (
                "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B",
                base,
                3,
                crate::records::DESIGN_MODULE_SKETCH,
                &[800],
            ),
            (
                "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB",
                base,
                11,
                "Geometry",
                &[100, 200, 300, 400, 700],
            ),
            (
                crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.0,
                base,
                crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.1,
                crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.2,
                &[101, 201, 301, 401, 701],
            ),
        ],
        records,
    )
}

pub(super) fn generated_act_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    let entity_1 = [1];
    let entity_2 = [2];
    let entity_7 = [7];
    let entity_9 = [9];
    let types = [
        (
            "00000000-0000-0000-0000-000000000100",
            "",
            1,
            "ACT",
            &entity_2[..],
        ),
        (
            "00000000-0000-0000-0000-000000000101",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000102",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000103",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000104",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000105",
            "",
            1,
            "ACT",
            &entity_7[..],
        ),
        (
            "00000000-0000-0000-0000-000000000106",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000107",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000108",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000109",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-00000000010a",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-00000000010b",
            "",
            1,
            "ACT",
            &entity_9[..],
        ),
        (
            "00000000-0000-0000-0000-00000000010c",
            "",
            1,
            "ACT",
            &entity_1[..],
        ),
    ];
    segment_metastream(
        "FusionACTSegmentType",
        "FusionACTSegmentType",
        "Fusion",
        &types,
        records,
    )
}

pub(super) fn generated_act_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    fn lp_ascii(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units: Vec<u16> = value.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
    let mut out = Vec::new();
    let mut records = vec![(2, out.len() as u64)];
    lp_ascii(&mut out, "256");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(b"decoy:ACTTable");
    out.extend_from_slice(&[0; 6]);
    records.push((1, out.len() as u64));
    lp_ascii(&mut out, "268");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    lp_ascii(&mut out, "ACTTable");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_utf16(&mut out, "0_985");
    lp_utf16(&mut out, "eeeeeeee-1111-2222-3333-ffffffffffff");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&9u32.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&2u32.to_le_bytes());
    for (name, guid) in [
        ("Appearance", "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb"),
        ("PhysicalMaterial", "cccccccc-1111-2222-3333-dddddddddddd"),
    ] {
        lp_ascii(&mut out, name);
        lp_utf16(&mut out, guid);
    }
    records.push((9, out.len() as u64));
    lp_ascii(&mut out, "267");
    out.extend_from_slice(&9u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 10]);
    out.push(1);
    out.extend_from_slice(&12u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_utf16(&mut out, "0_3");
    out.push(1);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 5]);
    out.push(1);
    out.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut out, "(Unsaved)");
    out.push(0);
    out.push(1);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    records.push((7, out.len() as u64));
    lp_ascii(&mut out, "261");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 10]);
    out.extend_from_slice(&2u32.to_le_bytes());
    for (name, guid) in [
        ("Appearance", "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb"),
        ("PhysicalMaterial", "cccccccc-1111-2222-3333-dddddddddddd"),
    ] {
        lp_ascii(&mut out, name);
        lp_utf16(&mut out, guid);
    }
    lp_utf16(&mut out, "0_985");
    (out, records)
}

pub(super) fn generated_design_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    fn lp_utf16(out: &mut Vec<u8>, value: &str) {
        let units: Vec<u16> = value.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn reference(relation: &mut [u8], at: usize, target: u32) {
        relation[at] = 1;
        relation[at + 1..at + 9].copy_from_slice(&u64::from(target).to_le_bytes());
    }

    fn push_reference(out: &mut Vec<u8>, target: u64) {
        out.push(1);
        out.extend_from_slice(&target.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
    }

    fn close_current_point(out: &mut Vec<u8>, paired_reference: u32, owner_reference: u32) {
        out.extend_from_slice(&[0; 16]);
        out.push(1);
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(&1.0f32.to_le_bytes());
        out.extend_from_slice(&1.0f32.to_le_bytes());
        out.extend_from_slice(&[0, 1, 0, 0, 0]);
        push_reference(out, u64::from(paired_reference));
        push_reference(out, u64::from(owner_reference));
    }

    fn push_point_companion(
        out: &mut Vec<u8>,
        class_tag: &str,
        record_index: u32,
        point_record_index: u32,
    ) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&record_index.to_le_bytes());
        out.extend_from_slice(&[0; 15]);
        push_reference(out, u64::from(point_record_index));
    }

    let mut out = Vec::new();
    let mut records = vec![(899, 0)];
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"263");
    out.extend_from_slice(&899u32.to_le_bytes());
    out.extend_from_slice(&[0; crate::design::body::GENERATED_BODY_MAP_ZERO_PREFIX_LEN]);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&42u64.to_le_bytes());
    out.extend_from_slice(&985u64.to_le_bytes());
    out.extend_from_slice(&1793u64.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    lp_utf16(&mut out, "BREP.synthetic.smbh");
    records.push((
        985,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let node_guid = "ABCD0000-1111-8222-A333-444444444444";
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"256");
    out.extend_from_slice(&985u64.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    lp_utf16(&mut out, "0_985");
    lp_utf16(&mut out, node_guid);
    out.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    lp_utf16(&mut out, "99999999-8888-8777-A666-555555555555");
    lp_utf16(
        &mut out,
        crate::design::presentation::PHYSICAL_MATERIAL_LIBRARY_ID,
    );
    lp_utf16(&mut out, "PrismMaterial-018");
    push_reference(&mut out, 7);
    out.push(0);
    push_reference(&mut out, 986);
    lp_utf16(&mut out, "Body");
    out.extend_from_slice(&1.0f32.to_le_bytes());
    out.extend_from_slice(&[1, 1]);
    lp_utf16(&mut out, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut out, crate::design::presentation::APPEARANCE_LIBRARY_ID);
    lp_utf16(&mut out, "Prism-001");
    records.push((
        900,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"260");
    out.extend_from_slice(&900u32.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    lp_utf16(&mut out, node_guid);
    out.extend_from_slice(&[0, 1, 1]);
    out.extend_from_slice(&985u64.to_le_bytes());
    records.push((
        277,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"257");
    out.extend_from_slice(&277u64.to_le_bytes());
    out.extend_from_slice(&[0u8; 5]);
    out.push(1);
    out.extend_from_slice(&[0u8; 4]);
    lp_utf16(&mut out, "0_277");
    out.extend_from_slice(&584u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&2u32.to_le_bytes());
    for reference in [33u32, 44] {
        out.push(1);
        out.extend_from_slice(&reference.to_le_bytes());
        out.extend_from_slice(&[0u8; 6]);
    }
    for (class_tag, record_index, members) in
        [("259", 33u32, [100u32, 200u32]), ("259", 44, [300, 400])]
    {
        records.push((
            u64::from(record_index),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        // A relation is `u8 1`, the counted `(reference, relation ordinal)`
        // pairs, the property-block presence byte, `ParentNode`, the u64
        // mask, the counted return run, and one zero byte.
        let mut relation = vec![0u8; 101];
        relation[0..4].copy_from_slice(&3u32.to_le_bytes());
        relation[4..7].copy_from_slice(class_tag.as_bytes());
        relation[7..11].copy_from_slice(&record_index.to_le_bytes());
        relation[19] = 1;
        relation[20..24].copy_from_slice(&2u32.to_le_bytes());
        reference(&mut relation, 24, members[0]);
        reference(&mut relation, 39, members[1]);
        reference(&mut relation, 55, 277);
        let state = if record_index == 33 { 0x10u64 } else { 0x04 };
        relation[66..74].copy_from_slice(&state.to_le_bytes());
        relation[74..78].copy_from_slice(&2u32.to_le_bytes());
        reference(&mut relation, 78, members[1]);
        reference(&mut relation, 89, members[0]);
        out.extend_from_slice(&relation);
    }
    for (record_index, persistent_id, coordinates) in [
        (100u32, 500u64, [1.25f64, -2.5f64]),
        (200, 501, [3.0, 4.0]),
        (300, 502, [-1.0, 0.5]),
        (400, 503, [2.0, 1.0]),
    ] {
        records.push((
            u64::from(record_index),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        let paired_reference = record_index + 1;
        let mut point = vec![0u8; 105];
        point[0..4].copy_from_slice(&3u32.to_le_bytes());
        point[4..7].copy_from_slice(b"266");
        point[7..11].copy_from_slice(&record_index.to_le_bytes());
        point[20] = 1;
        point[21..25].copy_from_slice(&1u32.to_le_bytes());
        point[25..29].copy_from_slice(&6u32.to_le_bytes());
        point[29..35].copy_from_slice(b"pt_tag");
        point[35..39].copy_from_slice(&23u32.to_le_bytes());
        point[39..62].copy_from_slice(b"IntrinsicMetaTypeuint64");
        point[62..70].copy_from_slice(&persistent_id.to_le_bytes());
        point[70] = 1;
        point[71..75].copy_from_slice(&paired_reference.to_le_bytes());
        point[89..97].copy_from_slice(&coordinates[0].to_le_bytes());
        point[97..105].copy_from_slice(&coordinates[1].to_le_bytes());
        close_current_point(&mut point, paired_reference, 277);
        out.extend_from_slice(&point);
        records.push((
            u64::from(paired_reference),
            u64::try_from(out.len()).expect("synthetic Design record offset"),
        ));
        push_point_companion(&mut out, "267", paired_reference, record_index);
    }
    records.push((
        600,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut curve = vec![0u8; 229];
    curve[0..4].copy_from_slice(&3u32.to_le_bytes());
    curve[4..7].copy_from_slice(b"264");
    curve[7..11].copy_from_slice(&600u32.to_le_bytes());
    curve[20] = 1;
    curve[21..25].copy_from_slice(&2u32.to_le_bytes());
    curve[25..29].copy_from_slice(&14u32.to_le_bytes());
    curve[29..43].copy_from_slice(b"crv_primary_id");
    curve[43..47].copy_from_slice(&23u32.to_le_bytes());
    curve[47..70].copy_from_slice(b"IntrinsicMetaTypeuint64");
    curve[70..78].copy_from_slice(&440u64.to_le_bytes());
    curve[78..82].copy_from_slice(&16u32.to_le_bytes());
    curve[82..98].copy_from_slice(b"crv_secondary_id");
    curve[98..102].copy_from_slice(&23u32.to_le_bytes());
    curve[102..125].copy_from_slice(b"IntrinsicMetaTypeuint64");
    curve[125..133].copy_from_slice(&0u64.to_le_bytes());
    for (ordinal, value) in [
        1.0f64,
        2.0,
        0.0,
        0.0,
        0.0,
        1.0,
        1.0,
        0.0,
        0.0,
        3.0,
        0.0,
        std::f64::consts::PI,
    ]
    .into_iter()
    .enumerate()
    {
        let offset = 133 + ordinal * 8;
        curve[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    push_reference(&mut curve, 277);
    out.extend_from_slice(&curve);
    records.push((
        700,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut alternate_point = vec![0u8; 157];
    alternate_point[0..4].copy_from_slice(&3u32.to_le_bytes());
    alternate_point[4..7].copy_from_slice(b"266");
    alternate_point[7..11].copy_from_slice(&700u32.to_le_bytes());
    alternate_point[20] = 1;
    alternate_point[21..25].copy_from_slice(&2u32.to_le_bytes());
    alternate_point[25..29].copy_from_slice(&13u32.to_le_bytes());
    alternate_point[29..42].copy_from_slice(b"EntityGenesis");
    alternate_point[42..46].copy_from_slice(&23u32.to_le_bytes());
    alternate_point[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_point[69..77].copy_from_slice(&9u64.to_le_bytes());
    alternate_point[77..81].copy_from_slice(&6u32.to_le_bytes());
    alternate_point[81..87].copy_from_slice(b"pt_tag");
    alternate_point[87..91].copy_from_slice(&23u32.to_le_bytes());
    alternate_point[91..114].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_point[114..122].copy_from_slice(&600u64.to_le_bytes());
    alternate_point[122] = 1;
    alternate_point[123..127].copy_from_slice(&701u32.to_le_bytes());
    alternate_point[141..149].copy_from_slice(&(-4.0f64).to_le_bytes());
    alternate_point[149..157].copy_from_slice(&5.0f64.to_le_bytes());
    close_current_point(&mut alternate_point, 701, 277);
    out.extend_from_slice(&alternate_point);
    records.push((
        701,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    push_point_companion(&mut out, "267", 701, 700);

    records.push((
        800,
        u64::try_from(out.len()).expect("synthetic Design record offset"),
    ));
    let mut alternate_curve = vec![0u8; 443];
    alternate_curve[0..4].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[4..7].copy_from_slice(b"265");
    alternate_curve[7..11].copy_from_slice(&800u32.to_le_bytes());
    alternate_curve[20] = 1;
    alternate_curve[21..25].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[25..29].copy_from_slice(&13u32.to_le_bytes());
    alternate_curve[29..42].copy_from_slice(b"EntityGenesis");
    alternate_curve[42..46].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[69..77].copy_from_slice(&10u64.to_le_bytes());
    alternate_curve[77..81].copy_from_slice(&14u32.to_le_bytes());
    alternate_curve[81..95].copy_from_slice(b"crv_primary_id");
    alternate_curve[95..99].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[99..122].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[122..130].copy_from_slice(&700u64.to_le_bytes());
    alternate_curve[130..134].copy_from_slice(&16u32.to_le_bytes());
    alternate_curve[134..150].copy_from_slice(b"crv_secondary_id");
    alternate_curve[150..154].copy_from_slice(&23u32.to_le_bytes());
    alternate_curve[154..177].copy_from_slice(b"IntrinsicMetaTypeuint64");
    alternate_curve[177..185].copy_from_slice(&0u64.to_le_bytes());
    alternate_curve[185..193].copy_from_slice(&42u64.to_le_bytes());
    alternate_curve[193..197].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[197..200].copy_from_slice(b"365");
    alternate_curve[200..204].copy_from_slice(&800u32.to_le_bytes());
    alternate_curve[273] = 1;
    alternate_curve[275..279].copy_from_slice(&2u32.to_le_bytes());
    alternate_curve[279..287].copy_from_slice(&1.0e-9f64.to_le_bytes());
    alternate_curve[287..291].copy_from_slice(&6u32.to_le_bytes());
    alternate_curve[291..295].copy_from_slice(&6u32.to_le_bytes());
    alternate_curve[295..299].copy_from_slice(&8u32.to_le_bytes());
    for (ordinal, knot) in [0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0].into_iter().enumerate() {
        let offset = 299 + ordinal * 8;
        alternate_curve[offset..offset + 8].copy_from_slice(&knot.to_le_bytes());
    }
    alternate_curve[347..351].copy_from_slice(&0u32.to_le_bytes());
    alternate_curve[351..355].copy_from_slice(&0u32.to_le_bytes());
    alternate_curve[355..359].copy_from_slice(&8u32.to_le_bytes());
    alternate_curve[359..363].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[363..367].copy_from_slice(&3u32.to_le_bytes());
    alternate_curve[367..371].copy_from_slice(&8u32.to_le_bytes());
    for (ordinal, coordinate) in [0.0f64, 0.0, 0.0, 1.0, 2.0, 0.0, 3.0, 1.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let offset = 371 + ordinal * 8;
        alternate_curve[offset..offset + 8].copy_from_slice(&coordinate.to_le_bytes());
    }
    push_reference(&mut alternate_curve, 277);
    out.extend_from_slice(&alternate_curve);
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(b"BodiesRoot");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&10u32.to_le_bytes());
    out.extend_from_slice(b"BodiesRoot");
    out.extend_from_slice(&2u32.to_le_bytes());
    for entity_suffix in [985u64, 8422] {
        out.push(1);
        out.extend_from_slice(&entity_suffix.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.push(0);
    let mut recipe_prefix = vec![0u8; 27];
    recipe_prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
    recipe_prefix[4..7].copy_from_slice(b"322");
    recipe_prefix[11..15].copy_from_slice(&123i32.to_le_bytes());
    recipe_prefix[23..27].copy_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&recipe_prefix);
    out.extend_from_slice(b"body_recipe_data");
    out.extend_from_slice(&(-1i64).to_le_bytes());
    for value in [2i32, 0, -1, 1, -1] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(b"pt_tag");
    out.extend_from_slice(&23u32.to_le_bytes());
    out.extend_from_slice(b"IntrinsicMetaTypeuint64");
    out.extend_from_slice(&439u64.to_le_bytes());
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"419");
    out.extend_from_slice(&4645u32.to_le_bytes());
    out.extend_from_slice(&[0; 14]);
    out.extend_from_slice(&19u32.to_le_bytes());
    out.extend_from_slice(b"EDGE_REFERENCE_LOST");
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"419");
    out.extend_from_slice(&4646u32.to_le_bytes());
    out.extend_from_slice(b"body_recipe_data");
    (out, records)
}

pub(super) fn generated_instance_properties_for(guid: &str) -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let mut logical = b"\x80\x00\x01\x00".to_vec();
    lp(&mut logical, "GenericSchema");
    lp(&mut logical, guid);
    lp(&mut logical, "Prism-001");
    lp(&mut logical, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    let value_block = logical.len();
    logical.resize(value_block + 209, 0);
    for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
        let offset = value_block + 112 + ordinal * 8;
        logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    logical[value_block + 171..value_block + 175].copy_from_slice(b"\x0c\x00\x00\x00");
    logical[value_block + 175..value_block + 183].copy_from_slice(&0.25f64.to_le_bytes());
    logical[value_block + 197..value_block + 201].copy_from_slice(b"\x0c\x00\x00\x00");
    logical[value_block + 201..value_block + 209].copy_from_slice(&1.5f64.to_le_bytes());

    paged_instance_properties(&logical)
}

pub(super) fn generated_prism_instance_properties(schema: &str, guid: &str) -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    let mut logical = b"\x80\x00\x01\x00".to_vec();
    lp(&mut logical, schema);
    lp(&mut logical, guid);
    lp(&mut logical, "Prism-001");
    lp(&mut logical, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    let position = logical.len();
    match schema {
        "PrismOpaqueSchema" => {
            logical.resize(position + 96, 0);
            for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
                let offset = position + 8 + ordinal * 8;
                logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            logical[position + 64..position + 68].copy_from_slice(b"\x0e\x20\x00\x00");
            logical[position + 68..position + 76].copy_from_slice(&0.25f64.to_le_bytes());
        }
        "PrismTransparentSchema" => {
            logical.resize(position + 177, 0);
            for (ordinal, value) in [0.1f64, 0.2, 0.3, 1.0].into_iter().enumerate() {
                let offset = position + 121 + ordinal * 8;
                logical[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            logical[position + 169..position + 177].copy_from_slice(&1.5f64.to_le_bytes());
        }
        _ => panic!("unsupported generated Prism schema"),
    }
    paged_instance_properties(&logical)
}

pub(super) fn paged_instance_properties(logical: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(0x88u32).to_le_bytes());
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let first = logical.len().min(132);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&logical[..first]);
    bytes.resize(16 + 136, 0);
    let mut rest = &logical[first..];
    while rest.len() > 128 {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"\x80\x00\x00\x00");
        bytes.extend_from_slice(&rest[..128]);
        rest = &rest[128..];
    }
    if !rest.is_empty() {
        bytes.extend_from_slice(&[0xff; 4]);
        bytes.extend_from_slice(&(rest.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(rest);
        let page_end = 16 + (bytes.len() - 16).next_multiple_of(136);
        bytes.resize(page_end, 0);
    }
    bytes
}

pub(super) fn generated_schema_from_paged(properties: &[u8]) -> &str {
    let length = u32::from_le_bytes(properties[24..28].try_into().unwrap()) as usize;
    std::str::from_utf8(&properties[28..28 + length]).unwrap()
}

pub(super) fn generated_definition_catalog_for(schema: &str) -> Vec<u8> {
    fn lp(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    let mut out = b"\x80\x00\x01\x00".to_vec();
    for value in [schema, "Prism-001", "Default", "Plastic/Thermoplastic"] {
        lp(&mut out, value);
    }
    out
}

pub(super) fn push_tagged_f64(b: &mut Vec<u8>, v: f64) {
    b.push(0x06);
    b.extend_from_slice(&v.to_le_bytes());
}

/// Push a `tag`-prefixed little-endian i64 (used for `0x04` longs and `0x15`
/// enum values in B-spline block fixtures).
pub(super) fn push_tagged_i64(b: &mut Vec<u8>, tag: u8, v: i64) {
    b.push(tag);
    b.extend_from_slice(&v.to_le_bytes());
}

/// Assemble a synthetic `.f3d` ZIP with a manifest, a BREP `.smbh`, a `.smb`
/// snapshot, and a few side entries, mirroring the spec's naming families.
pub(super) fn synthetic_f3d(include_smbh: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);

    let folder = "FusionAssetName[Active]";
    write_synthetic_manifests(&mut zip, stored);

    if include_smbh {
        zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smbh"), deflated)
            .unwrap();
        zip.write_all(&synthetic_smbh()).unwrap();
    }

    // A history-less .smb (header only, no history partition).
    let mut smb = synthetic_smbh();
    smb[39..47].copy_from_slice(&2u64.to_le_bytes());
    smb.truncate(60); // header prefix only, no history partition
    zip.start_file(format!("{folder}/Breps.BlobParts/Body1.smb"), stored)
        .unwrap();
    zip.write_all(&smb).unwrap();

    zip.start_file(
        format!("{folder}/FusionDesignSegmentType1/BulkStream.dat"),
        stored,
    )
    .unwrap();
    zip.write_all(b"design-bulk").unwrap();

    zip.start_file(format!("{folder}/Previews/thumbnail.png"), stored)
        .unwrap();
    zip.write_all(b"\x89PNG").unwrap();

    let cursor = zip.finish().unwrap();
    cursor.into_inner()
}

pub(super) fn synthetic_legacy_multi_brep_f3d() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let folder = "FusionAssetName[Active]";
    write_synthetic_manifests(&mut zip, stored);
    for name in ["first", "second"] {
        let mut smb = synthetic_smbh();
        smb[39..47].copy_from_slice(&2u64.to_le_bytes());
        smb.truncate(60);
        zip.start_file(format!("{folder}/Breps.BlobParts/BREP.{name}.smb"), stored)
            .unwrap();
        zip.write_all(&smb).unwrap();
    }
    for stream in ["BulkStream.dat", "MetaStream.dat"] {
        zip.start_file(format!("{folder}/Design1/{stream}"), stored)
            .unwrap();
        zip.write_all(b"legacy-design").unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub(super) fn synthetic_multi_asset_f3d(include_design_brep: bool) -> Vec<u8> {
    const DESIGN_GUID: &str = "10000000-0000-4000-8000-000000000001";
    const SIBLING_GUID: &str = "20000000-0000-4000-8000-000000000002";
    const SECONDARY_GUID: &str = "30000000-0000-4000-8000-000000000003";

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    zip.start_file("Manifest.dat", stored).unwrap();
    zip.write_all(
        &crate::manifest::encode_top_level(DESIGN_GUID, &["Simulation", "DesignAsset"]).unwrap(),
    )
    .unwrap();
    zip.start_file("Simulation/Manifest.dat", stored).unwrap();
    zip.write_all(
        &crate::manifest::encode_asset_header(
            "Simulation",
            SIBLING_GUID,
            SECONDARY_GUID,
            "SimulationAssetType",
        )
        .unwrap(),
    )
    .unwrap();
    zip.start_file("Simulation/FusionDesignSegmentType1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(b"sibling Design-name decoy").unwrap();
    zip.start_file("Simulation/Breps.BlobParts/BREP.sibling.smbh", stored)
        .unwrap();
    zip.write_all(&synthetic_smbh()).unwrap();
    zip.start_file("DesignAsset[Active]/Manifest.dat", stored)
        .unwrap();
    zip.write_all(&crate::manifest::encode_design_asset("DesignAsset", DESIGN_GUID).unwrap())
        .unwrap();
    zip.start_file(
        "DesignAsset[Active]/FusionDesignSegmentType1/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(b"selected Design stream").unwrap();
    zip.start_file(
        "DesignAsset[Active]/FusionNonDesignSegmentType1/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(b"selected-folder name decoy").unwrap();
    if include_design_brep {
        let mut design_brep = synthetic_geometry_smbh();
        design_brep[39..47].copy_from_slice(&2u64.to_le_bytes());
        zip.start_file(
            "DesignAsset[Active]/Breps.BlobParts/BREP.design.smb",
            stored,
        )
        .unwrap();
        zip.write_all(&design_brep).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn asm_header_parses_documented_fields() {
    let bytes = synthetic_smbh();
    let h = asm_header::parse(&bytes).expect("magic present");
    assert_eq!(h.width, 8);
    assert_eq!(h.save_format_version, Some(23100));
    assert_eq!(h.entity_count, Some(7));
    assert_eq!(h.flags, Some(3));
    assert_eq!(h.save_format_major(), Some(231));
    assert_eq!(h.save_format_minor(), Some(0));
    assert!(h.has_history_partition());
    // Flags `3` is the history bit plus revision `1` in bits 1 to 7. Nothing is
    // left over, so no bit reaches the uninterpreted set.
    assert_eq!(h.format_revision(), Some(1));
    assert_eq!(h.unassigned_flags(), Some(0));
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.product_version.as_deref(), Some("ASM 231.6.3.65535 OSX"));
    assert_eq!(h.save_date.as_deref(), Some("Tue Mar 31 16:16:19 2026"));
    assert_eq!(h.scale, Some(60.0));
    assert_eq!(h.linear, Some(1e-6));
    assert_eq!(h.angular, Some(1e-10));
}

/// Flag bits 1 to 7 hold the save format's revision number
/// ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#1-asm-binary-header)):
/// save format 22300 carries revision 2 and 22500 carries revision 3. Those
/// bits are assigned, so they leave the uninterpreted set; bits 8 and above
/// stay in it.
#[test]
fn asm_header_flag_bits_one_to_seven_hold_the_format_revision() {
    let header = |flags: u64| cadmpeg_asm::kernel_header::KernelHeader {
        width: 8,
        save_format_version: Some(22500),
        record_count: None,
        entity_count: None,
        flags: Some(flags),
        product_family: None,
        product_version: None,
        save_date: None,
        scale: None,
        linear: None,
        angular: None,
    };

    assert_eq!(header(0b0000_0101).format_revision(), Some(2));
    assert_eq!(header(0b0000_0111).format_revision(), Some(3));
    assert_eq!(header(0b1111_1110).format_revision(), Some(0x7f));
    assert_eq!(header(0b0000_0001).format_revision(), Some(0));

    assert_eq!(header(0b0000_0111).unassigned_flags(), Some(0));
    assert_eq!(header(0b1111_1111).unassigned_flags(), Some(0));
    assert_eq!(header(0x1_00).unassigned_flags(), Some(0x1_00));
    assert!(header(0b0000_0101).has_history_partition());
    assert!(!header(0b0000_0100).has_history_partition());
}

#[test]
fn asm_header_absent_on_non_asm_bytes() {
    assert!(asm_header::parse(b"not an asm stream at all").is_none());
    assert!(!asm_header::has_asm_magic(b"PK\x03\x04"));
}

/// The `BinaryFile4` fixed header ([spec §1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#1-asm-binary-header)): 15-byte magic, four little-endian
/// u32 words (save-format version, record count, entity count, flags), then the same
/// tagged string/double sequence as `BinaryFile8`.
pub(super) fn bf4_header_prefix(flags: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"ASM BinaryFile4");
    b.extend_from_slice(&22700u32.to_le_bytes()); // ACIS save-format version
    b.extend_from_slice(&0u32.to_le_bytes()); // record count (unwritten)
    b.extend_from_slice(&2u32.to_le_bytes()); // entity count
    b.extend_from_slice(&flags.to_le_bytes());
    push_u8_string(&mut b, "Autodesk Neutron");
    push_u8_string(&mut b, "ASM 227.5.0.65535 NT");
    push_u8_string(&mut b, "Mon Aug  8 02:39:24 2022");
    push_tagged_f64(&mut b, 50.0); // scale
    push_tagged_f64(&mut b, 1e-6); // resabs
    push_tagged_f64(&mut b, 1e-10); // resnor
    b
}

/// The minimal `BinaryFile4` active model slice: the planar-face graph of
/// `synthetic_geometry_smbh` with 4-byte integer/ref fields, the ASM-227
/// `lump` head for the body-subdivision record, and one edge resting on an
/// ellipse arc whose stored range is negative.
pub(super) fn synthetic_geometry_bf4_smbh() -> Vec<u8> {
    synthetic_geometry_bf4_smbh_with_arc_sense(0x0b)
}

pub(super) fn synthetic_geometry_bf4_nurbs_smbh() -> Vec<u8> {
    fn tagged_i32(bytes: &mut Vec<u8>, tag: u8, value: i32) {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let mut bytes = synthetic_geometry_bf4_smbh();
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let limit = asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 4).unwrap();
    let ellipse_range = records[19].offset..records[19].offset + records[19].len;

    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    tagged_i32(&mut curve, 0x0c, -1);
    tagged_i32(&mut curve, 0x04, -1);
    tagged_i32(&mut curve, 0x0c, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "surf_surf_int_cur");
    curve.extend_from_slice(b"\x0d\x04nubs");
    tagged_i32(&mut curve, 0x04, 2);
    tagged_i32(&mut curve, 0x15, 0);
    tagged_i32(&mut curve, 0x04, 2);
    for (knot, multiplicity) in [(0.0, 2), (1.0, 2)] {
        push_tagged_f64(&mut curve, knot);
        tagged_i32(&mut curve, 0x04, multiplicity);
    }
    for point in [[0.0, 0.0, 0.0], [0.5, 0.5, 0.0], [1.0, 0.0, 0.0]] {
        for coordinate in point {
            push_tagged_f64(&mut curve, coordinate);
        }
    }
    t_dbl(&mut curve, 0.0005);
    curve.push(0x10);
    t_end(&mut curve);
    bytes.splice(ellipse_range, curve);
    bytes
}

/// `synthetic_geometry_bf4_smbh` with the arc edge's sense byte set to
/// `arc_edge_sense` (`0x0b` forward, `0x0a` reversed).
pub(super) fn synthetic_geometry_bf4_smbh_with_arc_sense(arc_edge_sense: u8) -> Vec<u8> {
    // Width-4 writers; the remaining tag writers are width-independent.
    fn t_ref(b: &mut Vec<u8>, v: i32) {
        b.push(0x0c);
        b.extend_from_slice(&v.to_le_bytes());
    }
    fn t_long(b: &mut Vec<u8>, v: i32) {
        b.push(0x04);
        b.extend_from_slice(&v.to_le_bytes());
    }

    // Indices: 0 asmheader, 1 body, 2 lump, 3 shell, 4 face, 5 loop,
    // 6 plane, 7/8/9 coedges, 10/11/12 edges, 13/14/15 vertices,
    // 16/17/18 points, 19 ellipse.
    let mut r = Vec::new();

    // 0: asmheader
    t_ident(&mut r, "asmheader");
    push_u8_string(&mut r, "227.5.0.65535");
    t_end(&mut r);

    // 1: body  (chunk3 = first_lump)
    t_ident(&mut r, "body");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, 42); // 1 native ASM body key
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, 2); // 3 first_lump
    t_ref(&mut r, -1); // 4 wire
    t_ref(&mut r, -1); // 5 transform
    t_end(&mut r);

    // 2: lump  (chunk4 = first_shell, chunk5 = owner_body)
    t_ident(&mut r, "lump");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, 3); // 4 first_shell
    t_ref(&mut r, 1); // 5 owner_body
    t_end(&mut r);

    // 3: shell  (chunk5 = first_face, chunk7 = owner_lump)
    t_ident(&mut r, "shell");
    t_ref(&mut r, -1); // 0 next
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 null
    t_ref(&mut r, -1); // 4 null
    t_ref(&mut r, 4); // 5 first_face
    t_ref(&mut r, -1); // 6 wire
    t_ref(&mut r, 2); // 7 owner_lump
    t_end(&mut r);

    // 4: face
    t_ident(&mut r, "face");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_face
    t_ref(&mut r, 5); // 4 first_loop
    t_ref(&mut r, 3); // 5 owner_shell
    t_ref(&mut r, -1); // 6 null
    t_ref(&mut r, 6); // 7 surface
    r.push(0x0b); // 8 sense = forward
    r.push(0x0b); // 9 sides = single
    t_end(&mut r);

    // 5: loop
    t_ident(&mut r, "loop");
    t_ref(&mut r, -1); // 0 attrib
    t_long(&mut r, -1); // 1 history
    t_ref(&mut r, -1); // 2 null
    t_ref(&mut r, -1); // 3 next_loop
    t_ref(&mut r, 7); // 4 first_coedge
    t_ref(&mut r, 4); // 5 owner_face
    t_end(&mut r);

    // 6: plane-surface
    t_subident(&mut r, "plane");
    t_ident(&mut r, "surface");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_pos(&mut r, [0.0, 0.0, 0.0]);
    t_vec(&mut r, [0.0, 0.0, 1.0]);
    t_vec(&mut r, [1.0, 0.0, 0.0]);
    r.push(0x0b);
    t_end(&mut r);

    // 7/8/9: coedges forming the ring 7 -> 8 -> 9 -> 7
    let coedges = [(8i32, 9, 10), (9, 7, 11), (7, 8, 12)];
    for (next, prev, edge) in coedges {
        t_ident(&mut r, "coedge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, next); // 3 next
        t_ref(&mut r, prev); // 4 prev
        t_ref(&mut r, -1); // 5 partner
        t_ref(&mut r, edge); // 6 edge
        r.push(0x0b); // 7 sense = forward
        t_ref(&mut r, 5); // 8 owner_loop
        t_long(&mut r, 0); // 9 reserved
        t_ref(&mut r, -1); // 10 pcurve
        t_end(&mut r);
    }

    // 10/11/12: edges. Edge 10 rests on the ellipse arc (19) with the stored
    // ASM range [-π, -π/2]; edges 11/12 carry no curve.
    let edges = [(13i32, 14, 19), (14, 15, -1), (15, 13, -1)];
    for (start, end, curve) in edges {
        t_ident(&mut r, "edge");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, start); // 3 start_vertex
        t_dbl(&mut r, -std::f64::consts::PI); // 4 t_start
        t_ref(&mut r, end); // 5 end_vertex
        t_dbl(&mut r, -std::f64::consts::FRAC_PI_2); // 6 t_end
        t_ref(&mut r, -1); // 7 owner_coedge
        t_ref(&mut r, curve); // 8 curve
        r.push(if curve >= 0 { arc_edge_sense } else { 0x0b }); // 9 sense
        push_u8_string(&mut r, "unknown"); // 10 continuity text
        t_end(&mut r);
    }

    // 13/14/15: vertices
    let verts = [(10i32, 16), (11, 17), (12, 18)];
    for (edge, point) in verts {
        t_ident(&mut r, "vertex");
        t_ref(&mut r, -1); // 0 attrib
        t_long(&mut r, -1); // 1 history
        t_ref(&mut r, -1); // 2 null
        t_ref(&mut r, edge); // 3 owning_edge
        t_long(&mut r, 0); // 4 index_flag
        t_ref(&mut r, point); // 5 point
        t_end(&mut r);
    }

    // 16/17/18: points
    let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for p in points {
        t_ident(&mut r, "point");
        t_ref(&mut r, -1);
        t_long(&mut r, -1);
        t_ref(&mut r, -1);
        t_pos(&mut r, p);
        t_end(&mut r);
    }

    // 19: ellipse-curve (circle: ratio 1) carrying edge 10's arc.
    t_subident(&mut r, "ellipse");
    t_ident(&mut r, "curve");
    t_ref(&mut r, -1); // attrib
    t_long(&mut r, -1); // history
    t_ref(&mut r, -1); // null
    t_pos(&mut r, [0.5, 0.0, 0.0]); // center
    t_vec(&mut r, [0.0, 0.0, 1.0]); // normal
    t_vec(&mut r, [0.5, 0.0, 0.0]); // major axis (radius 0.5 cm)
    t_dbl(&mut r, 1.0); // ratio
    t_end(&mut r);

    // History boundary.
    t_ident(&mut r, "delta_state");

    let mut out = bf4_header_prefix(5);
    out.extend_from_slice(&r);
    out
}

#[test]
fn asm_header_parses_binaryfile4_fields() {
    let bytes = bf4_header_prefix(5);
    assert!(asm_header::has_asm_magic(&bytes));
    let h = asm_header::parse(&bytes).expect("magic present");
    assert_eq!(h.width, 4);
    assert_eq!(h.save_format_version, Some(22700));
    assert_eq!(h.record_count, Some(0));
    assert_eq!(h.entity_count, Some(2));
    assert_eq!(h.flags, Some(5));
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.product_version.as_deref(), Some("ASM 227.5.0.65535 NT"));
    assert_eq!(h.save_date.as_deref(), Some("Mon Aug  8 02:39:24 2022"));
    assert_eq!(h.scale, Some(50.0));
    assert_eq!(h.linear, Some(1e-6));
    assert_eq!(h.angular, Some(1e-10));
    // The record stream begins directly after the tolerance doubles.
    assert_eq!(asm_header::record_stream_start(&bytes), Some(bytes.len()));
}

#[test]
fn decodes_binaryfile4_geometry_with_lump_topology() {
    let f3d = f3d_with_smbh(&synthetic_geometry_bf4_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.bodies.len(), 1);
    // The ASM-227 `lump` head is emitted as the region record.
    assert_eq!(result.ir().model.regions.len(), 1);
    assert_eq!(result.ir().model.shells.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.points.len(), 3);

    // The circle arc's stored [-π, -π/2] range is wrapped into the canonical
    // [0, τ] domain with its sweep preserved.
    let arc = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.is_some())
        .expect("edge on the ellipse carrier");
    let [start, end] = arc.param_range.expect("arc range");
    assert!((start - std::f64::consts::PI).abs() < 1e-9);
    assert!((end - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}

#[test]
fn generated_f3d_rewrites_binaryfile4_geometry() {
    let source = f3d_with_smbh(&synthetic_geometry_bf4_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated BinaryFile4 decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.points[0].position.x += 2.5;
    let expected = edited.model.points[0].position;
    let edge = edited
        .model
        .edges
        .iter_mut()
        .find(|edge| edge.curve.is_some())
        .expect("generated BinaryFile4 arc edge");
    let range = edge.param_range.as_mut().expect("generated arc range");
    range[0] += 0.125;
    range[1] -= 0.125;
    let expected_range = *range;
    edited.model.faces[0].sense = match edited.model.faces[0].sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    };
    let expected_face_sense = edited.model.faces[0].sense;

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("generated BinaryFile4 regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated BinaryFile4 decode");
    assert_eq!(round_trip.ir().model.points[0].position, expected);
    assert_eq!(
        round_trip
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge.curve.is_some())
            .and_then(|edge| edge.param_range),
        Some(expected_range)
    );
    assert_eq!(round_trip.ir().model.faces[0].sense, expected_face_sense);
}

#[test]
fn generated_f3d_rewrites_binaryfile4_nurbs_integer_fields() {
    let source = f3d_with_smbh(&synthetic_geometry_bf4_nurbs_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated BinaryFile4 NURBS decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| {
            matches!(
                curve.geometry,
                cadmpeg_ir::geometry::CurveGeometry::Nurbs(_)
            )
        })
        .expect("generated BinaryFile4 NURBS curve");
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &mut curve.geometry else {
        unreachable!()
    };
    nurbs.degree = 1;
    nurbs.periodic = true;
    nurbs.knots = vec![-1.0, -1.0, 2.0, 2.0, 2.0];
    nurbs.control_points[1].z = 4.5;
    let expected = nurbs.clone();

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("generated BinaryFile4 NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated BinaryFile4 NURBS decode");
    assert!(round_trip.ir().model.curves.iter().any(|curve| {
        matches!(&curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) if nurbs == &expected)
    }));
}

#[test]
fn reversed_edge_sense_reverses_its_conic_carrier() {
    let f3d = f3d_with_smbh(&synthetic_geometry_bf4_smbh_with_arc_sense(0x0a));
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    // A reversed edge runs `E(t) = C(-t)`; the IR keeps edges forward on
    // their curve, so the conic carrier is emitted with a negated plane
    // normal. The stored parameters already live on the reversed
    // parameterization and transform exactly like a forward edge's.
    let arc = result
        .ir()
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.is_some())
        .expect("edge on the ellipse carrier");
    let [start, end] = arc.param_range.expect("arc range");
    assert!((start - std::f64::consts::PI).abs() < 1e-9);
    assert!((end - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);

    let curve_id = arc.curve.as_ref().expect("curve link");
    let carrier = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("conic carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Circle { axis, .. } = &carrier.geometry else {
        panic!("expected the ratio-1 ellipse to decode as a circle");
    };
    assert!((axis.z - -1.0).abs() < 1e-12, "axis must be negated");
}

#[test]
fn delta_state_boundary_is_located_at_an_exact_identifier() {
    let bytes = synthetic_smbh();
    let off = asm_header::solved_record_limit(&bytes).expect("has a delta_state");
    assert_eq!(&bytes[off..off + 2], &[0x0d, 0x0b]);
    assert_eq!(&bytes[off + 2..off + 13], b"delta_state");

    // The header flag is part of the partition contract. A history-less
    // `.smb` with the same solved prefix has no partition boundary.
    let mut smb = bytes;
    smb[39..47].copy_from_slice(&2u64.to_le_bytes());
    smb.truncate(off);
    assert!(asm_header::solved_record_limit(&smb).is_none());
}

#[test]
fn history_preamble_record_is_the_modern_partition_boundary() {
    let direct = synthetic_smbh();
    let delta = asm_header::solved_record_limit(&direct).unwrap();
    let mut bytes = direct[..delta].to_vec();
    let expected = bytes.len();
    t_ident(&mut bytes, "Begin-of-ASM-History-Data");
    t_end(&mut bytes);
    bytes.extend_from_slice(&direct[delta..]);

    assert_eq!(asm_header::solved_record_limit(&bytes), Some(expected));
    let start = asm_header::record_stream_start(&bytes).unwrap();
    let solved = cadmpeg_asm::sab::frame(&bytes, start, expected, 8).unwrap();
    assert_eq!(
        solved.last().map(|record| record.name.as_str()),
        Some("body")
    );
}

#[test]
fn delta_state_text_inside_a_payload_cannot_cut_the_solved_stream() {
    let direct = synthetic_smbh();
    let delta = asm_header::solved_record_limit(&direct).unwrap();
    let start = asm_header::record_stream_start(&direct).unwrap();
    let mut bytes = direct[..start].to_vec();
    t_ident(&mut bytes, "metadata");
    push_u8_string(&mut bytes, "delta_state");
    t_end(&mut bytes);
    let expected = bytes.len();
    bytes.extend_from_slice(&direct[delta..]);

    assert_eq!(asm_header::solved_record_limit(&bytes), Some(expected));
}

#[test]
fn decode_retains_generated_asm_history_graph() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert_eq!(f3d_native(result.ir()).asm_histories.len(), 1);
    let history = &f3d_native(result.ir()).asm_histories[0];
    assert_eq!(history.stream_size, Some(2));
    assert_eq!(history.history_entry_count, Some(99));
    assert_eq!(history.states.len(), 2);
    assert_eq!(history.states[0].state_id, 2);
    assert_eq!(history.states[0].next_ref, Some(1));
    assert_eq!(history.states[0].bulletin_boards.len(), 1);
    assert_eq!(history.states[0].bulletin_boards[0].changes.len(), 2);
    assert_eq!(history.states[0].records.len(), 1);
    assert_eq!(history.states[0].records[0].name, "history_payload");
    assert_eq!(history.states[0].records[0].revision_id, Some(1830));
    assert_eq!(history.states[0].records[0].entity_references, [1830, -1]);
    assert!(!history.states[0].records[0].raw_bytes.is_empty());
    assert_eq!(
        history.states[0].bulletin_boards[0].changes[1].kind,
        crate::history_records::AsmEntityChangeKind::Insert
    );
    assert_eq!(history.states[1].previous_ref, Some(0));
    assert_eq!(history.states[1].next_ref, None);
    assert!(result.report().geometry_transferred);
}

#[test]
fn generated_f3d_rewrites_fixed_delta_state_header() {
    let source = f3d_with_smbh(&synthetic_geometry_with_history_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated history decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        let history = &mut native.asm_histories[0];
        assert!(history.byte_offset > 0);
        assert!(history.states[0].byte_offset > 0);
        history.stream_size = Some(8);
        history.history_entry_count = Some(120);
        history.states[0].state_id = 8;
        history.states[0].version_flag = 4;
        history.states[0].state_flag = 6;
        history.states[0].previous_ref = Some(12);
        history.states[0].next_ref = Some(14);
        history.states[0].node_index = 16;
        history.states[0].partner_ref = Some(18);
        history.states[0].owner_ref = 20;
        let board = &mut history.states[0].bulletin_boards[0];
        assert!(board.byte_offset > 0);
        board.owner_ref = 22;
        board.number = 24;
        assert!(board.changes[0].byte_offset > 0);
        board.changes[0].kind = crate::history_records::AsmEntityChangeKind::Delete;
        board.changes[0].old_ref = Some(26);
        board.changes[0].new_ref = None;
        board.changes[1].new_ref = Some(28);
    });

    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, &fidelity, &mut regenerated)
        .expect("delta-state owner regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated history decode");
    let state = &f3d_native(round_trip.ir()).asm_histories[0].states[0];
    assert_eq!(
        f3d_native(round_trip.ir()).asm_histories[0].stream_size,
        Some(8)
    );
    assert_eq!(
        f3d_native(round_trip.ir()).asm_histories[0].history_entry_count,
        Some(120)
    );
    assert_eq!(state.state_id, 8);
    assert_eq!(state.version_flag, 4);
    assert_eq!(state.state_flag, 6);
    assert_eq!(state.previous_ref, Some(12));
    assert_eq!(state.next_ref, Some(14));
    assert_eq!(state.node_index, 16);
    assert_eq!(state.partner_ref, Some(18));
    assert_eq!(state.owner_ref, 20);
    let board = &state.bulletin_boards[0];
    assert_eq!(board.owner_ref, 22);
    assert_eq!(board.number, 24);
    assert_eq!(
        board.changes[0].kind,
        crate::history_records::AsmEntityChangeKind::Delete
    );
    assert_eq!(board.changes[0].old_ref, Some(26));
    assert_eq!(board.changes[0].new_ref, None);
    assert_eq!(board.changes[1].new_ref, Some(28));
}

#[test]
fn classify_matches_spec_families() {
    assert_eq!(classify("a/Breps.BlobParts/x.smbh"), role::BREP_SMBH);
    assert_eq!(classify("a/Breps.BlobParts/x.smb"), role::BREP_SMB);
    assert_eq!(
        classify("a/ProteinAssets.BlobParts/y.protein"),
        role::PROTEIN
    );
    assert_eq!(classify("a/Design1/BulkStream.dat"), role::BULKSTREAM);
    assert_eq!(classify("a/Design1/MetaStream.dat"), role::METASTREAM);
    assert_eq!(classify("Manifest.dat"), role::MANIFEST);
    assert_eq!(classify("a/Previews/thumb.png"), role::PREVIEW);
    assert_eq!(classify("a/x.paramesh"), role::PARAMESH);
    assert_eq!(classify("a/b/"), role::DIRECTORY);
}

use crate::container::classify;

#[test]
fn detect_high_on_f3d_zip_low_on_bare_zip() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    assert_eq!(codec.detect(&f3d), Confidence::High);

    // A ZIP whose visible prefix has no f3d markers.
    let mut bare = zip::ZipWriter::new(Cursor::new(Vec::new()));
    bare.start_file(
        "readme.txt",
        crate::zip_write::file_options(CompressionMethod::Stored),
    )
    .unwrap();
    bare.write_all(b"hello").unwrap();
    let bare = bare.finish().unwrap().into_inner();
    assert_eq!(codec.detect(&bare), Confidence::Low);

    assert_eq!(codec.detect(b"\x00\x01\x02\x03 not a zip"), Confidence::No);
}

#[test]
fn inspect_enumerates_and_reads_headers() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    let mut cur = Cursor::new(f3d);
    let summary = codec.inspect(&mut cur, &InspectOptions::default()).unwrap();

    assert_eq!(summary.format, "f3d");
    assert_eq!(summary.container_kind, "zip");

    let smbh = summary
        .entries
        .iter()
        .find(|e| e.role == role::BREP_SMBH)
        .expect("smbh entry present");
    assert_eq!(smbh.compression, "deflate");
    assert_eq!(
        smbh.attributes.get("product_family").map(String::as_str),
        Some("Autodesk Neutron")
    );
    assert_eq!(smbh.attributes.get("scale").map(String::as_str), Some("60"));
    assert!(smbh.attributes.contains_key("history_partition_offset"));
    assert!(smbh.attributes.contains_key("sha256"));

    // The header identifies the unique history-bearing stream.
    assert!(summary
        .notes
        .iter()
        .any(|n| n.contains("history-bearing BREP")));
}

#[test]
fn decode_refuses_when_max_entities_is_below_archive_entry_cardinality() {
    use cadmpeg_core::decode::ResourceDimension;

    let mut options = DecodeOptions::default();
    options.policy.limits.max_entities = 1;
    let error = F3dCodec
        .decode(&mut Cursor::new(synthetic_f3d(true)), &options)
        .expect_err("max_entities below archive entry count must refuse at admission");
    assert!(
        matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::Entities
        ),
        "{error:?}"
    );
}

#[test]
fn decode_yields_metadata_and_honest_report() {
    let codec = F3dCodec;
    let f3d = synthetic_f3d(true);
    let mut cur = Cursor::new(f3d);
    let result = codec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(!result.report().geometry_transferred);
    assert!(result.ir().model.faces.is_empty());
    assert!(result.report().error_count() >= 1);
    assert!(result.report().losses.iter().any(|l| matches!(
        l.code.category(),
        cadmpeg_ir::report::LossCategory::Geometry
    )));

    let unknowns = result.ir().native_unknowns("f3d").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(result.source_fidelity().retained_records.len(), 2);
    assert!(result
        .source_fidelity()
        .retained_records
        .iter()
        .all(|record| record.sha256.len() == 64));
    assert!(result
        .source_fidelity()
        .retained_record("f3d:file:source-image#0")
        .is_some());
    let source = result.ir().source.as_ref().expect("source metadata");
    assert_eq!(source.format, "f3d");
    assert_eq!(
        source.attributes.get("product_family").map(String::as_str),
        Some("Autodesk Neutron")
    );
    // resabs/resnor were carried into tolerances.
    assert_eq!(result.ir().tolerances.linear, 1e-6);
    assert_f3d_native_parity(result.ir());
    assert!(result
        .source_fidelity()
        .annotations
        .provenance
        .contains_key(&unknowns[0].id.0));
}

#[test]
fn smb_only_is_an_explicit_geometry_fallback_without_history() {
    let f3d = synthetic_f3d(false);
    with_scan(&f3d, |scan| {
        let fallback = container::select_fallback_brep(scan).unwrap();
        assert!(!fallback.is_smbh);
        assert!(container::select_history_brep(scan).is_none());
        assert!(container::legacy_design_model_breps(scan).is_none());
        let summary = container::summarize(scan);
        assert!(summary
            .notes
            .iter()
            .any(|note| note.contains("no BREP header declares a history partition")));
    });
}

#[test]
fn legacy_design_segment_selects_its_complete_brep_set() {
    let f3d = synthetic_legacy_multi_brep_f3d();
    with_scan(&f3d, |scan| {
        assert!(container::select_fallback_brep(scan).is_none());
        let selected = container::legacy_design_model_breps(scan).unwrap();
        assert_eq!(selected.len(), 2);
        assert!(selected[0].name.ends_with("BREP.first.smb"));
        assert!(selected[1].name.ends_with("BREP.second.smb"));
    });
}

#[test]
fn manifest_selects_design_asset_independently_of_brep_order() {
    let f3d = synthetic_multi_asset_f3d(true);
    with_scan(&f3d, |scan| {
        assert_eq!(scan.design_asset_folder(), Some("DesignAsset[Active]"));
        assert_eq!(scan.breps.len(), 2);
        let design_breps = container::design_breps(scan).collect::<Vec<_>>();
        assert_eq!(design_breps.len(), 1);
        assert!(design_breps[0].name.ends_with("BREP.design.smb"));
        assert!(container::select_history_brep(scan).is_none());
        assert_eq!(
            container::select_fallback_brep(scan).map(|brep| brep.name.as_str()),
            Some("DesignAsset[Active]/Breps.BlobParts/BREP.design.smb")
        );
        let streams = scan
            .entries
            .iter()
            .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            streams,
            ["DesignAsset[Active]/FusionDesignSegmentType1/BulkStream.dat"]
        );
    });
}

#[test]
fn manifest_selects_brep_less_design_asset() {
    let f3d = synthetic_multi_asset_f3d(false);
    with_scan(&f3d, |scan| {
        assert_eq!(scan.design_asset_folder(), Some("DesignAsset[Active]"));
        assert_eq!(scan.breps.len(), 1);
        assert_eq!(container::design_breps(scan).count(), 0);
        assert!(container::select_fallback_brep(scan).is_none());
        assert!(container::select_history_brep(scan).is_none());
    });
}

#[test]
fn decode_uses_manifest_selected_geometry_not_the_first_brep_asset() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(synthetic_multi_asset_f3d(true)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(decoded.report().geometry_transferred);
    assert_eq!(decoded.ir().model.bodies.len(), 1);
    assert_eq!(
        decoded
            .ir()
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("asset_folder"))
            .map(String::as_str),
        Some("DesignAsset[Active]")
    );
    assert!(decoded
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| !body.id.0.contains("BREP.sibling")));
}

#[test]
fn decode_does_not_use_a_sibling_brep_for_a_brep_less_design_asset() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(synthetic_multi_asset_f3d(false)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == LossCode::shared(LossTaxonomy::MissingGeometryStream)
            && loss.message == "no ASM BREP stream (.smb/.smbh) was found in the container"
    }));
}

#[test]
fn smbh_header_string_region_starts_at_byte_47() {
    // Regression: the three product strings begin at byte 47, not 48 — the
    // schema word `7` at offset 40 puts its low byte 0x07 at offset 47, which
    // doubles as the first string's TAG_UTF8_U8 tag. A parser that starts the
    // string walk at 48 reads a length byte as a tag and desyncs the whole
    // header, so record_stream_start lands mid-header and framing fails.
    let prefix = smbh_header_prefix();
    assert_eq!(prefix[47], 0x07, "first string tag at offset 47");
    // The header parses all three strings and both tolerances despite the
    // overlap, and the record stream begins immediately after the last double.
    let h = asm_header::parse(&prefix).expect("magic present");
    assert_eq!(h.product_family.as_deref(), Some("Autodesk Neutron"));
    assert_eq!(h.flags, Some(3));
    assert_eq!(h.angular, Some(1e-10));
    assert_eq!(
        asm_header::record_stream_start(&prefix),
        Some(prefix.len()),
        "record stream starts right after the header"
    );
}

#[test]
fn sab_framer_indexes_records_from_asmheader() {
    let bytes = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&bytes).expect("record stream start");
    let limit = asm_header::solved_record_limit(&bytes).unwrap_or(bytes.len());
    let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).expect("framing succeeds");

    // asmheader occupies index 0; the topology records follow in order.
    assert_eq!(records[0].index, 0);
    assert_eq!(records[0].head, "asmheader");
    assert_eq!(records[1].head, "body");
    assert_eq!(records[4].head, "face");
    assert_eq!(records[4].name, "face");
    assert_eq!(records[6].name, "plane-surface");
    // The face's surface reference (chunk[7]) resolves to the plane at index 6.
    assert_eq!(records[4].ref_at(7), Some(6));
    assert!(records.iter().all(|r| r.head != "delta_state"));
}

#[test]
fn decode_builds_valid_topology_and_geometry() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::Point3;

    let f3d = f3d_with_smbh(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred);
    assert!(result
        .report()
        .notes
        .iter()
        .all(|note| !note.starts_with("container-level inspection only")));
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    let ownerships = f3d_native(result.ir()).vertex_ownerships;
    assert_eq!(ownerships.len(), 3);
    assert_eq!(
        ownerships
            .iter()
            .map(|metadata| metadata.endpoint_index)
            .collect::<Vec<_>>(),
        [0, 1, 0]
    );
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(f3d_native(result.ir()).face_sidedness.len(), 1);
    assert_eq!(f3d_native(result.ir()).face_sidedness[0].containment, None);
    let continuities = f3d_native(result.ir()).edge_continuities;
    assert_eq!(continuities.len(), 3);
    assert!(continuities
        .iter()
        .all(|metadata| metadata.continuity == "unknown"));
    assert!(continuities
        .iter()
        .all(|metadata| metadata.sense == cadmpeg_ir::topology::Sense::Forward));
    assert_f3d_native_parity(result.ir());
    assert!(result
        .source_fidelity()
        .annotations
        .provenance
        .contains_key(&result.ir().model.bodies[0].id.0));

    // The plane decoded with its stored origin and complete parameter frame.
    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            assert_eq!(*origin, Point3::new(0.0, 0.0, 0.0));
            assert_eq!(normal.z, 1.0);
            assert_eq!(*u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected plane, got {other:?}"),
    }
    // Point coordinates converted centimetre → millimetre (×10).
    let xs: Vec<f64> = result
        .ir()
        .model
        .points
        .iter()
        .map(|p| p.position.x)
        .collect();
    assert!(xs.contains(&10.0));

    // The decoded document is internally valid: refs resolve, the loop ring
    // closes, no bounds violations.
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);

    // Edges carry no analytic curve (their carriers were null), which is legal.
    assert!(result.ir().model.edges.iter().all(|e| e.curve.is_none()));
    // The loop's coedge ring is the three coedges in order.
    assert_eq!(result.ir().model.loops[0].coedges.len(), 3);
}

#[test]
fn history_topology_decode_matches_full_brep_graph() {
    for bytes in [
        synthetic_geometry_with_pcurve_smbh(),
        synthetic_full_rolling_ball_smbh("rb_blend_spl_sur"),
    ] {
        let start = asm_header::record_stream_start(&bytes).expect("record stream start");
        let limit = asm_header::solved_record_limit(&bytes).expect("solved record limit");
        let records = cadmpeg_asm::sab::frame(&bytes, start, limit, 8).expect("frame BREP");

        let full = crate::history::historical_topology(
            &crate::brep::decode(&records, &bytes, "full", crate::ids::ID_FORMAT).asm,
        )
        .expect("full topology");
        let history = crate::history::historical_topology(
            &crate::brep::decode_history_topology(&records, &bytes, crate::ids::ID_FORMAT).asm,
        )
        .expect("history topology");

        assert_eq!(history, full);
    }
}

#[test]
fn decode_transfers_generated_wire_body_topology() {
    let mut result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(result.ir().model.shells.len(), 1);
    assert!(result.ir().model.shells[0].faces.is_empty());
    assert_eq!(result.ir().model.shells[0].wire_edges.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 2);
    assert_eq!(result.ir().model.points.len(), 2);
    assert_eq!(result.ir().model.curves.len(), 1);
    assert_eq!(f3d_native(result.ir()).wire_topologies.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).wire_topologies[0].side,
        cadmpeg_asm::brep::records::WireSide::Out
    );
    assert_eq!(
        result.ir().model.shells[0].wire_edges[0],
        result.ir().model.edges[0].id
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("wire=")));
    update_f3d_native(result.ir_mut(), |native| {
        native.wire_topologies[0].side = cadmpeg_asm::brep::records::WireSide::In;
    });
    let mut edited = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(result.ir(), result.source_fidelity(), &mut edited)
        .expect("wire-side retained edit");
    let edited = F3dCodec
        .decode(&mut Cursor::new(edited), &DecodeOptions::default())
        .expect("wire-side retained round trip");
    assert_eq!(
        f3d_native(edited.ir()).wire_topologies[0].side,
        cadmpeg_asm::brep::records::WireSide::In
    );
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_transfers_isolated_vertex_wire_topology() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert!(result.ir().model.shells[0].wire_edges.is_empty());
    assert_eq!(result.ir().model.shells[0].free_vertices.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(
        result.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
    );
    assert!(f3d_native(result.ir()).vertex_ownerships.is_empty());
    let wire = &f3d_native(result.ir()).wire_topologies[0];
    assert!(wire.edges.is_empty());
    assert_eq!(
        wire.free_vertex,
        Some(result.ir().model.vertices[0].id.clone())
    );
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "free-vertex findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_classifies_generated_mixed_face_wire_body_as_general() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    assert_eq!(
        result.ir().model.bodies.len(),
        1,
        "mixed decode report: {:?}",
        result.report()
    );
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.shells[0].wire_edges.len(), 1);
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.curves.len(), 1);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "mixed-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_degenerate_curve_decodes_regenerates_and_writes_source_less() {
    use cadmpeg_ir::{geometry::CurveGeometry, math::Point3};

    let source = f3d_with_smbh(&synthetic_geometry_with_degenerate_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated degenerate curve decode");
    let curve = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| matches!(curve.geometry, CurveGeometry::Degenerate { .. }))
        .expect("degenerate curve carrier");
    assert_eq!(
        curve.geometry,
        CurveGeometry::Degenerate {
            point: Point3::new(0.0, 0.0, 0.0)
        }
    );
    let curve_id = curve.id.clone();

    let mut edited = decoded.ir().clone();
    let edited_curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .expect("editable degenerate curve");
    edited_curve.geometry = CurveGeometry::Degenerate {
        point: Point3::new(2.0, 3.0, 4.0),
    };
    let mut regenerated = Vec::new();
    F3dCodec
        .write_preserved_with_source_fidelity(&edited, decoded.source_fidelity(), &mut regenerated)
        .expect("degenerate curve regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated degenerate curve decode");
    assert!(regenerated.ir().model.curves.iter().any(|curve| {
        curve.geometry
            == CurveGeometry::Degenerate {
                point: Point3::new(2.0, 3.0, 4.0),
            }
    }));

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let expected = CurveGeometry::Degenerate {
        point: Point3::new(0.0, 0.0, 0.0),
    };
    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less degenerate curve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less degenerate curve round trip");
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == expected));
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "degenerate-curve findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_general_face_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less general body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less general body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.shells[0].wire_edges.len(), 1);
    assert_eq!(round_trip.ir().model.edges.len(), 4);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "mixed-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_general_face_and_point_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    let free = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let renamed = free
        .ir()
        .to_canonical_json()
        .expect("canonical free-vertex JSON")
        .replace("f3d:brep:", "generated:general_point_wire:");
    let mut free =
        cadmpeg_ir::document::CadIr::from_json(&renamed).expect("renamed free-vertex IR");
    source_less.model.shells[0]
        .free_vertices
        .push(free.model.vertices[0].id.clone());
    source_less.model.vertices.append(&mut free.model.vertices);
    source_less.model.points.append(&mut free.model.points);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less face-and-point-wire body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less face-and-point-wire body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.shells[0].wire_edges.len(), 1);
    assert_eq!(round_trip.ir().model.shells[0].free_vertices.len(), 1);
    assert_eq!(f3d_native(round_trip.ir()).wire_topologies.len(), 2);
    assert!(f3d_native(round_trip.ir())
        .wire_topologies
        .iter()
        .any(|wire| wire.edges.is_empty() && wire.free_vertex.is_some()));
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "face-and-point-wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_solid_and_wire_bodies_together() {
    let mut source_less = cadmpeg_ir::examples::unit_cube();
    let decoded_wire = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let wire_json = decoded_wire
        .ir()
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:combined_wire:");
    let mut wire =
        cadmpeg_ir::document::CadIr::from_json(&wire_json).expect("renamed combined wire IR");
    source_less.model.bodies.append(&mut wire.model.bodies);
    source_less.model.regions.append(&mut wire.model.regions);
    source_less.model.shells.append(&mut wire.model.shells);
    source_less.model.edges.append(&mut wire.model.edges);
    source_less.model.vertices.append(&mut wire.model.vertices);
    source_less.model.points.append(&mut wire.model.points);
    source_less.model.curves.append(&mut wire.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less solid-plus-wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less solid-plus-wire round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 2);
    assert_eq!(
        round_trip
            .ir()
            .model
            .bodies
            .iter()
            .map(|body| body.kind)
            .collect::<Vec<_>>(),
        [
            cadmpeg_ir::topology::BodyKind::Solid,
            cadmpeg_ir::topology::BodyKind::Wire,
        ]
    );
    assert_eq!(round_trip.ir().model.faces.len(), 6);
    assert_eq!(round_trip.ir().model.shells[1].wire_edges.len(), 1);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "combined-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_wire_body_topology() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    update_f3d_native(&mut source_less, |native| {
        native.wire_topologies[0].side = cadmpeg_asm::brep::records::WireSide::In;
    });
    let expected_curve = source_less.model.curves[0].geometry.clone();
    let expected_points = source_less
        .model
        .points
        .iter()
        .map(|point| point.position)
        .collect::<Vec<_>>();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less wire body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less wire body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(round_trip.ir().model.shells[0].wire_edges.len(), 1);
    assert_eq!(
        f3d_native(round_trip.ir()).wire_topologies[0].side,
        cadmpeg_asm::brep::records::WireSide::In
    );
    assert_eq!(round_trip.ir().model.edges.len(), 1);
    assert_eq!(
        round_trip
            .ir()
            .model
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>(),
        expected_points
    );
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected_curve);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_isolated_vertex_wire() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    update_f3d_native(&mut source_less, |native| {
        native.wire_topologies[0].side = cadmpeg_asm::brep::records::WireSide::In;
    });

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less free-vertex wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less free-vertex wire round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert!(round_trip.ir().model.shells[0].wire_edges.is_empty());
    assert_eq!(round_trip.ir().model.shells[0].free_vertices.len(), 1);
    assert!(round_trip.ir().model.edges.is_empty());
    assert_eq!(round_trip.ir().model.vertices.len(), 1);
    assert_eq!(
        round_trip.ir().model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
    );
    assert!(f3d_native(round_trip.ir()).vertex_ownerships.is_empty());
    let wire = &f3d_native(round_trip.ir()).wire_topologies[0];
    assert!(wire.edges.is_empty());
    assert_eq!(
        wire.free_vertex,
        Some(round_trip.ir().model.vertices[0].id.clone())
    );
    assert_eq!(wire.side, cadmpeg_asm::brep::records::WireSide::In);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "free-vertex findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_edge_and_point_wires_on_one_shell() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let free = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let free_json = free
        .ir()
        .to_canonical_json()
        .expect("canonical free-vertex JSON");
    for namespace in ["generated:point_wire_one:", "generated:point_wire_two:"] {
        let renamed = free_json.replace("f3d:brep:", namespace);
        let mut free =
            cadmpeg_ir::document::CadIr::from_json(&renamed).expect("renamed free-vertex IR");
        source_less.model.shells[0]
            .free_vertices
            .push(free.model.vertices[0].id.clone());
        source_less.model.vertices.append(&mut free.model.vertices);
        source_less.model.points.append(&mut free.model.points);
    }

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less mixed-wire shell encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less mixed-wire shell round trip");
    assert_eq!(round_trip.ir().model.shells[0].wire_edges.len(), 1);
    assert_eq!(round_trip.ir().model.shells[0].free_vertices.len(), 2);
    assert_eq!(f3d_native(round_trip.ir()).wire_topologies.len(), 3);
    assert!(f3d_native(round_trip.ir())
        .wire_topologies
        .iter()
        .any(|wire| wire.edges.len() == 1 && wire.free_vertex.is_none()));
    assert!(f3d_native(round_trip.ir())
        .wire_topologies
        .iter()
        .any(|wire| wire.edges.is_empty() && wire.free_vertex.is_some()));
    assert_eq!(
        f3d_native(round_trip.ir())
            .wire_topologies
            .iter()
            .filter(|wire| wire.edges.is_empty() && wire.free_vertex.is_some())
            .count(),
        2
    );
    assert_eq!(round_trip.ir().model.vertices.len(), 4);
    assert_eq!(round_trip.ir().model.points.len(), 4);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "mixed-wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_two_independent_wire_bodies() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire IR");
    second.model.bodies[0].transform = Some(cadmpeg_ir::transform::Transform {
        rows: [
            [1.0, 0.0, 0.0, 25.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });
    source_less.model.bodies.append(&mut second.model.bodies);
    source_less.model.regions.append(&mut second.model.regions);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less two-wire-body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-wire-body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 2);
    assert!(round_trip
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.kind == cadmpeg_ir::topology::BodyKind::Wire));
    assert_eq!(round_trip.ir().model.regions.len(), 2);
    assert_eq!(round_trip.ir().model.shells.len(), 2);
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    assert_eq!(round_trip.ir().model.curves.len(), 2);
    assert_eq!(
        round_trip.ir().model.bodies[1]
            .transform
            .expect("second wire transform")
            .rows[0][3],
        25.0
    );
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_edge_wire_ring() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_edge_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire edge IR");
    let second_edge = second.model.edges[0].id.clone();
    source_less.model.shells[0].wire_edges.push(second_edge);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-edge wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-edge wire round trip");
    assert_eq!(round_trip.ir().model.shells[0].wire_edges.len(), 2);
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    assert_eq!(round_trip.ir().model.curves.len(), 2);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_region_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_region_two:");
    let mut second = cadmpeg_ir::document::CadIr::from_json(&second_json)
        .expect("renamed second wire region IR");
    let body_id = source_less.model.bodies[0].id.clone();
    let region_id = second.model.regions[0].id.clone();
    second.model.regions[0].body = body_id;
    source_less.model.bodies[0].regions.push(region_id);
    source_less.model.regions.append(&mut second.model.regions);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-region wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-region wire round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(round_trip.ir().model.bodies[0].regions.len(), 2);
    assert_eq!(round_trip.ir().model.regions.len(), 2);
    assert_eq!(round_trip.ir().model.shells.len(), 2);
    assert!(round_trip
        .ir()
        .model
        .regions
        .iter()
        .all(|region| region.body == round_trip.ir().model.bodies[0].id));
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_shell_wire_region() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less.set_native_unknowns("f3d", &[]).unwrap();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_shell_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire shell IR");
    let region_id = source_less.model.regions[0].id.clone();
    let shell_id = second.model.shells[0].id.clone();
    second.model.shells[0].region = region_id;
    source_less.model.regions[0].shells.push(shell_id);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &source_less,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-shell wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-shell wire round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(round_trip.ir().model.regions.len(), 1);
    assert_eq!(round_trip.ir().model.regions[0].shells.len(), 2);
    assert_eq!(round_trip.ir().model.shells.len(), 2);
    assert!(round_trip
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.region == round_trip.ir().model.regions[0].id));
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new());
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn analytic_carrier_decode_covers_each_shape() {
    use cadmpeg_asm::brep::geometry::{decode_curve, decode_surface};
    use cadmpeg_asm::sab::{Record, Token};
    use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

    fn rec(head: &str, tokens: Vec<Token>) -> Record {
        Record {
            index: 0,
            name: head.to_string(),
            head: head.to_string(),
            tokens: tokens.into(),
            offset: 0,
            len: 0,
        }
    }
    let refn = || Token::Ref(-1);
    let base = || vec![refn(), Token::Long(-1), refn()];

    // cone with sine==0 decodes to a cylinder; |major| (cm) ×10 = radius (mm).
    let mut cyl = base();
    cyl.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]), // axis
        Token::Vector3([2.0, 0.0, 0.0]), // ref × r_major, |.|=2 cm
        Token::Double(1.0),              // ratio
        Token::Double(0.0),              // sine → cylinder
        Token::Double(1.0),              // cosine
        Token::Double(2.0),              // r1 = 2 cm
    ]);
    match decode_surface(&rec("cone", cyl)).unwrap().0 {
        SurfaceGeometry::Cylinder {
            radius,
            axis,
            ref_direction,
            ..
        } => {
            assert_eq!(radius, 20.0);
            assert_eq!(axis.z, 1.0);
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected cylinder, got {other:?}"),
    }

    let mut elliptical_cylinder = base();
    elliptical_cylinder.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([2.0, 0.0, 0.0]),
        Token::Double(0.4),
        Token::Double(0.0),
        Token::Double(1.0),
        Token::Double(2.0),
    ]);
    assert!(matches!(
        decode_surface(&rec("cone", elliptical_cylinder)).unwrap().0,
        SurfaceGeometry::Cone {
            radius: 20.0,
            ratio: 0.4,
            half_angle: 0.0,
            ..
        }
    ));

    // cone with nonzero sine keeps the acute half-angle atan2(|sine|, |cosine|).
    // A both-negative sine/cosine pair has a positive slope (the radius still
    // grows along `+axis`, so the axis is kept), and the negative cosine
    // marks the inward native normal for the face-sense fold.
    let mut cone = base();
    cone.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([2.0, 0.0, 0.0]),
        Token::Double(1.0),
        Token::Double(-0.5), // sine (both-negative branch)
        Token::Double(-0.866_025_4),
        Token::Double(2.0),
    ]);
    let (geo, inward) = decode_surface(&rec("cone", cone)).unwrap();
    assert!(inward, "negative cosine points the native normal inward");
    match geo {
        SurfaceGeometry::Cone {
            half_angle,
            axis,
            ref_direction,
            ..
        } => {
            assert!((half_angle - 0.5f64.atan2(0.866_025_4)).abs() < 1e-12);
            assert_eq!(axis.z, 1.0, "positive slope keeps the axis");
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected cone, got {other:?}"),
    }

    // A negative sine with positive cosine shrinks the radius along the
    // native axis; the IR cone grows along `+axis`, so the axis flips. The
    // radius comes from the major-axis vector, not the trailing u-parameter
    // scale double, which diverges on offset-derived surfaces.
    let mut shrinking = base();
    shrinking.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([4.655, 0.0, 0.0]), // |major| = 4.655 cm
        Token::Double(1.0),
        Token::Double(-0.5), // sine
        Token::Double(0.866_025_4),
        Token::Double(5.055), // u-parameter scale, not the radius
    ]);
    let (geo, inward) = decode_surface(&rec("cone", shrinking)).unwrap();
    assert!(!inward, "positive cosine keeps the outward normal");
    match geo {
        SurfaceGeometry::Cone {
            half_angle,
            axis,
            radius,
            ..
        } => {
            assert!((half_angle - 0.5f64.atan2(0.866_025_4)).abs() < 1e-12);
            assert_eq!(axis.z, -1.0, "negative slope flips the axis");
            assert!((radius - 46.55).abs() < 1e-12);
        }
        other => panic!("expected cone, got {other:?}"),
    }

    // sphere: the signed radius identifies a concave carrier and is preserved.
    let mut sph = base();
    sph.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Double(-1.0), // concave
        Token::Vector3([1.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
    ]);
    let (geo, signed) = decode_surface(&rec("sphere", sph)).unwrap();
    assert!(!signed);
    match geo {
        SurfaceGeometry::Sphere {
            radius,
            axis,
            ref_direction,
            ..
        } => {
            assert_eq!(radius, -10.0);
            assert_eq!(axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected sphere, got {other:?}"),
    }

    // torus: major/minor ×10; signed minor radius is preserved.
    let mut tor = base();
    tor.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Double(1.0),  // major
        Token::Double(-2.0), // signed minor radius, with |minor| > major
        Token::Vector3([1.0, 0.0, 0.0]),
    ]);
    let (geo, inside_out) = decode_surface(&rec("torus", tor)).unwrap();
    assert!(!inside_out);
    match geo {
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ref_direction,
            ..
        } => {
            assert_eq!(major_radius, 10.0);
            assert_eq!(minor_radius, -20.0);
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected torus, got {other:?}"),
    }

    // ellipse with ratio 1 → circle; radius = |ref| (cm) ×10.
    let mut circ = base();
    circ.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([3.0, 0.0, 0.0]),
        Token::Double(1.0),
    ]);
    match decode_curve(&rec("ellipse", circ)).unwrap() {
        CurveGeometry::Circle { radius, .. } => assert_eq!(radius, 30.0),
        other => panic!("expected circle, got {other:?}"),
    }

    // ellipse with ratio != 1 → ellipse; minor = major·|ratio|.
    let mut ell = base();
    ell.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([4.0, 0.0, 0.0]),
        Token::Double(0.5),
    ]);
    match decode_curve(&rec("ellipse", ell)).unwrap() {
        CurveGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => {
            assert_eq!(major_radius, 40.0);
            assert_eq!(minor_radius, 20.0);
        }
        other => panic!("expected ellipse, got {other:?}"),
    }

    // straight line: origin ×10, unit direction.
    let mut line = vec![refn(), refn(), refn()];
    line.extend([
        Token::Position([1.0, 0.0, 0.0]),
        Token::Vector3([0.0, 1.0, 0.0]),
    ]);
    match decode_curve(&rec("straight", line)).unwrap() {
        CurveGeometry::Line { origin, direction } => {
            assert_eq!(origin.x, 10.0);
            assert_eq!(direction.y, 1.0);
        }
        other => panic!("expected line, got {other:?}"),
    }
}

#[test]
fn decode_succeeds_when_geometry_present() {
    let f3d = f3d_with_smbh(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.surfaces.len(), 1);
}

#[test]
fn decode_keeps_face_on_unknown_surface() {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    // Rename the plane so the face rests on an undecoded carrier.
    let mut smbh = synthetic_geometry_smbh();
    let needle = b"\x0e\x05plane";
    let pos = smbh
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("plane subident present");
    smbh[pos + 2..pos + 7].copy_from_slice(b"splne");

    let f3d = f3d_with_smbh(&smbh);
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.surfaces.len(), 1);

    let SurfaceGeometry::Unknown { record } = &result.ir().model.surfaces[0].geometry else {
        panic!("expected unknown surface geometry");
    };
    let link = record.as_ref().expect("unknown surface links to a record");
    assert!(
        result
            .ir()
            .native_unknowns("f3d")
            .unwrap()
            .iter()
            .any(|u| u.id == *link),
        "the linked unknown record is present in the arena"
    );

    let note = result
        .report()
        .losses
        .iter()
        .find(|l| l.message.contains("unknown-geometry surface"))
        .expect("unknown-surface loss note present");
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Warning);
    assert!(note.message.contains("Native kinds: splne=1."));

    // The decoded document still validates.
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn cached_unmodeled_spline_families_retain_exact_shape_and_opaque_construction() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};

    for family in [
        "crv_crv_v_bl_spl_sur",
        "crv_srf_v_bl_spl_sur",
        "sfcv_free_bl_spl_sur",
        "VBL_OFFSURF",
        "offsetvbsur",
        "skin_spl_sur2",
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_exact_spl_sur_smbh(family))),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} cached decode: {error}"));
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_)))
            .unwrap_or_else(|| panic!("{family} must retain its solved NURBS carrier"));
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.surface == surface.id)
            .unwrap_or_else(|| panic!("{family} must retain its construction identity"));
        let ProceduralSurfaceDefinition::Unknown {
            record: Some(record),
        } = &procedural.definition
        else {
            panic!("{family} must retain its opaque construction")
        };
        assert!(result
            .ir()
            .native_unknowns("f3d")
            .unwrap()
            .iter()
            .any(|unknown| unknown.id == *record));
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("unknown-geometry surface")));
    }
}

#[test]
fn decode_reports_faces_with_missing_surface_references() {
    for (surface, condition) in [(-1i64, "null-reference=1"), (999, "dangling-reference=1")] {
        let mut smbh = synthetic_mixed_smbh();
        let start = asm_header::record_stream_start(&smbh).unwrap();
        let limit = asm_header::solved_record_limit(&smbh).unwrap();
        let records = cadmpeg_asm::sab::frame(&smbh, start, limit, 8).unwrap();
        let face = records
            .iter()
            .filter(|record| record.head == "face")
            .nth(1)
            .expect("second generated face");
        let record = &mut smbh[face.offset..face.offset + face.len];
        let surface_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
        record[surface_ref + 1..surface_ref + 9].copy_from_slice(&surface.to_le_bytes());

        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("missing face surface remains an explicitly lossy decode");
        assert_eq!(result.ir().model.faces.len(), 1);
        let note = result
            .report()
            .losses
            .iter()
            .find(|loss| loss.message.contains("required surface reference"))
            .unwrap_or_else(|| {
                panic!(
                    "missing face-surface loss note: {:?}",
                    result.report().losses
                )
            });
        assert!(note.message.contains(condition), "{}", note.message);
    }
}

#[test]
fn decode_reports_undecoded_edge_curve_kinds() {
    let mut smbh = synthetic_geometry_with_procedural_curve_smbh();
    let needle = b"nubs";
    let position = smbh
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("procedural NURBS cache present");
    smbh[position] = b'x';

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("undecoded edge-curve carrier remains a successful topology decode");

    let note = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("no decodable inline B-spline cache"))
        .expect("undecoded edge-curve loss note");
    assert!(
        note.message.contains("Native kinds: intcurve=1."),
        "{}",
        note.message
    );
}

#[test]
fn decode_reports_dangling_edge_curve_references() {
    let mut smbh = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&smbh).unwrap();
    let limit = asm_header::solved_record_limit(&smbh).unwrap();
    let records = cadmpeg_asm::sab::frame(&smbh, start, limit, 8).unwrap();
    let edge = &records[10];
    let record = &mut smbh[edge.offset..edge.offset + edge.len];
    let curve_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[curve_ref + 1..curve_ref + 9].copy_from_slice(&999i64.to_le_bytes());

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("dangling curve reference remains a successful topology decode");
    let note = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("no decodable inline B-spline cache"))
        .expect("dangling edge-curve loss note");
    assert!(note.message.contains("Native kinds: dangling-reference=1."));
}
