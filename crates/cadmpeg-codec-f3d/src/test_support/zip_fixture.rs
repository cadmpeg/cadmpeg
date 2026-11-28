// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.f3d` ZIP archive builders.

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use zip::CompressionMethod;

use crate::container;
use crate::test_support::lost_edge_fixture::remove_body_lost_edge_bulkstream;
use crate::test_support::manifest_fixture::{
    write_synthetic_manifests, write_synthetic_manifests_with_version,
};
use crate::test_support::protein_fixture::{
    generated_definition_catalog_for, generated_instance_properties_for,
    generated_schema_from_paged,
};
use crate::test_support::smbh_geometry_fixture::synthetic_geometry_smbh;
use crate::test_support::smbh_header_fixture::synthetic_smbh;
use crate::test_support::smbh_revision_fixture::scrubbed_definition;
use crate::test_support::streams_fixture::{
    design_metastream_with_records, generated_act_bulkstream, generated_act_metastream,
    generated_design_base_feature_bulkstream, generated_design_base_feature_metastream,
    generated_design_base_flange_bulkstream, generated_design_base_flange_metastream,
    generated_design_bulkstream, generated_design_copy_paste_bodies_bulkstream,
    generated_design_copy_paste_bodies_metastream, generated_design_copy_paste_bulkstream,
    generated_design_copy_paste_metastream, generated_design_form_bulkstream,
    generated_design_form_metastream, generated_design_metastream,
    generated_design_remove_body_bulkstream, generated_design_remove_body_metastream,
    generated_design_sketch_dimension_bulkstream, generated_design_sketch_dimension_metastream,
    generated_design_surface_stitch_bulkstream, generated_design_surface_stitch_metastream,
    generated_design_types, GeneratedDesignMetastreamVariant,
};
use crate::F3dCodec;

pub(crate) fn with_scan<T>(bytes: &[u8], f: impl FnOnce(&container::ContainerScan<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let scan = container::scan(&ctx, root).unwrap();
    f(&scan)
}

pub(crate) fn assert_revision_surface_round_trip(smbh: Vec<u8>, expected_kind: &str) {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision surface decode");
    let procedural = result
        .ir()
        .geometry
        .procedural_surfaces
        .first()
        .expect("revision surface construction");
    let expected = scrubbed_definition(procedural.definition());
    let kind = json::to_value(procedural.definition()).expect("kind")["kind"]
        .as_str()
        .expect("kind string")
        .to_string();
    assert_eq!(kind, expected_kind);
    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            "f3d",
            &[],
        )
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None))
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less revision surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less revision surface round trip");
    let actual = scrubbed_definition(
        round_trip
            .ir()
            .geometry
            .procedural_surfaces
            .first()
            .expect("round-trip construction")
            .definition(),
    );
    assert_eq!(actual, expected);
}

/// Wrap an ASM stream byte blob into a `.f3d` ZIP as `Body1.smbh`.
pub(crate) fn f3d_with_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

/// The same archive as [`f3d_with_smbh`], with the top-level manifest
/// declaring `version`. The two archives differ in the version field alone.
pub(crate) fn f3d_with_smbh_and_manifest_version(smbh: &[u8], version: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests_with_version(&mut zip, stored, version);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

/// `archive` with `version` as the document version of its root
/// `Manifest.dat`, which leads that record as a length-prefixed string. The
/// other members are copied as stored.
pub(crate) fn with_manifest_version(archive: &[u8], version: &str) -> Vec<u8> {
    use std::io::Read;

    let session = cadmpeg_core::decode::test_support::desktop_decode_context();
    session
        .charge_input(
            cadmpeg_core::decode::u64_from_index(archive.len()),
            "the archive",
        )
        .unwrap();
    let mut source = zip::ZipArchive::new(Cursor::new(archive)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    for index in 0..source.len() {
        let name = source
            .by_index_raw(index)
            .unwrap()
            .name()
            .unwrap()
            .into_owned();
        if name != "Manifest.dat" {
            zip.raw_copy_file(source.by_index_raw(index).unwrap())
                .unwrap();
            continue;
        }
        let payload = if crate::zip_write::is_zstd_member(&mut source, index).unwrap() {
            let mut entry = source.by_index_raw(index).unwrap();
            crate::zip_write::expand_member(&session, &mut entry, &name, true).unwrap()
        } else {
            let mut payload = Vec::new();
            source
                .by_index(index)
                .unwrap()
                .read_to_end(&mut payload)
                .unwrap();
            payload
        };
        let length = usize::try_from(u32::from_le_bytes(payload[..4].try_into().unwrap())).unwrap();
        let mut manifest = u32::try_from(version.len()).unwrap().to_le_bytes().to_vec();
        manifest.extend_from_slice(version.as_bytes());
        manifest.extend_from_slice(&payload[4 + length..]);
        zip.start_file(name, stored).unwrap();
        zip.write_all(&manifest).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub(crate) fn f3d_with_deflated_smbh(smbh: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh",
        deflated,
    )
    .unwrap();
    zip.write_all(smbh).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(crate) fn set_zip_entry_uncompressed_size(archive: &mut [u8], target: &[u8], size: u32) {
    let central = archive
        .windows(4)
        .enumerate()
        .find_map(|(offset, signature)| {
            if signature != b"PK\x01\x02" || offset + 46 > archive.len() {
                return None;
            }
            let name_length = usize::from(u16::from_le_bytes(
                archive[offset + 28..offset + 30]
                    .try_into()
                    .expect("central name-length field"),
            ));
            (archive.get(offset + 46..offset + 46 + name_length) == Some(target)).then_some(offset)
        })
        .expect("generated ZIP central-directory entry");
    archive[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
}

pub(crate) fn f3d_with_configuration(smbh: &[u8], name: &str, payload: &[u8]) -> Vec<u8> {
    f3d_with_configurations(smbh, &[(name, payload)])
}

/// An archive of `smbh` with a configuration table document of each name and
/// payload of `documents`.
pub(crate) fn f3d_with_configurations(smbh: &[u8], documents: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    for (name, payload) in documents {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub(crate) fn f3d_with_smbh_and_protein(smbh: &[u8]) -> Vec<u8> {
    f3d_with_smbh_and_protein_guids(smbh, &["11111111-2222-3333-4444-555555555555"])
}

pub(crate) fn f3d_with_smbh_and_protein_guids(smbh: &[u8], guids: &[&str]) -> Vec<u8> {
    let properties = guids
        .iter()
        .map(|guid| generated_instance_properties_for(guid))
        .collect::<Vec<_>>();
    let (design_bulk, design_records) = generated_design_bulkstream();
    f3d_with_smbh_and_instance_properties_and_design(
        smbh,
        &properties,
        &design_bulk,
        &design_records,
    )
}

/// An archive whose Design stream also carries a browser-body appearance
/// record that names `override_visual` for the generated body.
pub(crate) fn f3d_with_smbh_and_protein_guids_and_browser_override(
    smbh: &[u8],
    guids: &[&str],
    override_visual: &str,
) -> Vec<u8> {
    let properties = guids
        .iter()
        .map(|guid| generated_instance_properties_for(guid))
        .collect::<Vec<_>>();
    let (mut design_bulk, design_records) = generated_design_bulkstream();
    let node_guid = "ABCD0000-1111-8222-A333-444444444444";
    for value in [
        "e966e81d-2581-4d41-821d-839938974425",
        node_guid,
        "DE897CF7-F483-4D31-A2D8-41671FE36D3D",
        "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
        "PrismMaterial-018",
        "ba2d3026-32c4-4584-b0e1-a738e387fa35",
        override_visual,
        crate::design::presentation::APPEARANCE_LIBRARY_ID,
        "Prism-090",
    ] {
        crate::test_support::lp_utf16(&mut design_bulk, value);
    }
    crate::test_support::lp_utf16(&mut design_bulk, node_guid);
    design_bulk.push(0);
    design_bulk.extend_from_slice(&[0x01, 0x01]);
    design_bulk.extend_from_slice(&985u64.to_le_bytes());
    f3d_with_smbh_and_instance_properties_and_design(
        smbh,
        &properties,
        &design_bulk,
        &design_records,
    )
}

pub(crate) fn f3d_with_smbh_and_instance_properties(
    smbh: &[u8],
    properties: &[Vec<u8>],
) -> Vec<u8> {
    let (design_bulk, design_records) = generated_design_bulkstream();
    f3d_with_smbh_and_instance_properties_and_design(
        smbh,
        properties,
        &design_bulk,
        &design_records,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_sketch_dimension(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_sketch_dimension_bulkstream();
    let design_metastream = generated_design_sketch_dimension_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_base_feature(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_base_feature_bulkstream();
    let design_metastream = generated_design_base_feature_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_base_flange(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_base_flange_bulkstream();
    let design_metastream = generated_design_base_flange_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_remove_body(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_remove_body_bulkstream();
    let design_metastream = generated_design_remove_body_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

/// The generated `RemoveBody` design whose construction group's identity
/// wrapper is followed by an entity-tracking path at an unrelated record index,
/// with both tracking records registered in the Design type table.
pub(crate) fn f3d_with_smbh_and_remove_body_tracking_path(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let mut types = generated_design_types(GeneratedDesignMetastreamVariant::RemoveBody);
    let (design_bulk, design_records, tracking_types) =
        crate::test_support::lost_edge_fixture::remove_body_tracking_path_bulkstream(types.len());
    let wrapper = [u64::from(
        crate::test_support::lost_edge_fixture::TRACKING_WRAPPER,
    )];
    let carrier = [u64::from(
        crate::test_support::lost_edge_fixture::TRACKING_WRAPPER + 1,
    )];
    let entities: [&[u64]; 2] = [&wrapper, &carrier];
    for ((guid, base, version), entities) in tracking_types.into_iter().zip(entities) {
        types.push((guid, base, version, "EntityTracking", entities));
    }
    let design_metastream = design_metastream_with_records(&types, &design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

/// The generated `RemoveBody` design whose two-operand construction group ends
/// an unresolved edge run of `run` records.
pub(crate) fn f3d_with_smbh_and_remove_body_lost_edge_run(smbh: &[u8], run: u32) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = remove_body_lost_edge_bulkstream(run);
    let design_metastream = generated_design_remove_body_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_surface_stitch(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_surface_stitch_bulkstream();
    let design_metastream = generated_design_surface_stitch_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_copy_paste(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_copy_paste_bulkstream();
    let design_metastream = generated_design_copy_paste_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_copy_paste_bodies(smbh: &[u8]) -> Vec<u8> {
    let (design_bulk, design_records) = generated_design_copy_paste_bodies_bulkstream();
    let design_metastream = generated_design_copy_paste_bodies_metastream(&design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &[],
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_form(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_form_bulkstream();
    let design_metastream = generated_design_form_metastream(&design_records);
    let tsm = generated_form_tsm();
    f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
        &[(
            "FusionAssetName[Active]/TSplines.BlobParts/TSpline.synthetic.tsm",
            &tsm,
        )],
    )
}

pub(crate) fn f3d_with_form_body_resource(tsm: &[u8]) -> Vec<u8> {
    use crate::test_support::{indexed_header, lp_utf16, push_marked_reference};
    let (mut bulk, mut records) = generated_design_form_bulkstream();
    let mut types = generated_design_types(GeneratedDesignMetastreamVariant::Form);
    let rows = [
        (
            "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571",
            "98542EB9-A4F2-4137-A808-DBB5B3CD6159",
            0,
            "Fusion",
            &[1700u64][..],
        ),
        (
            "FA1F0081-C9A1-4E14-8F25-D12DD868ED90",
            "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
            10,
            "Fusion",
            &[1701u64][..],
        ),
        (
            "22709494-9149-4A70-A829-7D748A7B5DB1",
            "1B7920A0-725B-4368-9448-E4D323D53E7C",
            6,
            "Fusion",
            &[1702u64][..],
        ),
        (
            "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA",
            "",
            4,
            "Fusion",
            &[1703u64][..],
        ),
        (
            "702B9CD2-537C-429E-8CC4-22BEEEB98C37",
            "",
            1,
            "Fusion",
            &[1704u64][..],
        ),
    ];
    let first = types.len();
    types.extend(rows);
    for (ordinal, record) in (1700u32..=1704).enumerate() {
        records.push((u64::from(record), u64::try_from(bulk.len()).unwrap()));
        let tag: [u8; 3] = format!("{:03}", first + ordinal + 256)
            .as_bytes()
            .try_into()
            .unwrap();
        indexed_header(&mut bulk, tag, record);
        match record {
            1700 => {
                bulk.extend_from_slice(&[0; 10]);
                lp_utf16(
                    &mut bulk,
                    "TSpline.00000000-0000-4000-8000-000000000099.tsm",
                );
                push_marked_reference(&mut bulk, 1702);
            }
            1701 => {
                for value in [
                    "00000000-0000-4000-8000-000000000001",
                    "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
                    "",
                    "PrismMaterial-001",
                ] {
                    lp_utf16(&mut bulk, value);
                }
                for target in [1400, 1703, 1702, 1704] {
                    push_marked_reference(&mut bulk, target);
                }
                indexed_header(&mut bulk, *b"258", record);
            }
            _ => bulk.extend_from_slice(&[0; 10]),
        }
    }
    let meta =
        crate::test_support::streams_fixture::design_metastream_with_records(&types, &records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
        &synthetic_geometry_smbh(),
        &[],
        &bulk,
        &meta,
        &[(
            "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000099.tsm",
            tsm,
        )],
    )
}

pub(crate) fn f3d_with_partial_form_body_resource_aliases(tsm: &[u8], literal: &str) -> Vec<u8> {
    use crate::test_support::{indexed_header, lp_ascii, lp_utf16, push_marked_reference};
    let (mut bulk, mut records) = generated_design_form_bulkstream();
    let mut types = generated_design_types(GeneratedDesignMetastreamVariant::Form);
    let rows = [
        (
            "830A2A2B-0AA9-4D6A-ACA1-F7A2B2A06571",
            "98542EB9-A4F2-4137-A808-DBB5B3CD6159",
            0,
            "Fusion",
            &[1700u64, 1710, 1720][..],
        ),
        (
            "FA1F0081-C9A1-4E14-8F25-D12DD868ED90",
            "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
            10,
            "Fusion",
            &[1701u64, 1711][..],
        ),
        (
            "22709494-9149-4A70-A829-7D748A7B5DB1",
            "1B7920A0-725B-4368-9448-E4D323D53E7C",
            6,
            "Fusion",
            &[1702u64, 1712][..],
        ),
        (
            "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA",
            "",
            4,
            "Fusion",
            &[1703u64][..],
        ),
        (
            "702B9CD2-537C-429E-8CC4-22BEEEB98C37",
            "",
            1,
            "Fusion",
            &[1704u64][..],
        ),
    ];
    let first = types.len();
    types.extend(rows);
    for (record, kind) in [
        (1700u32, 0usize),
        (1701, 1),
        (1702, 2),
        (1703, 3),
        (1704, 4),
        (1720, 0),
        (1710, 0),
        (1711, 1),
        (1712, 2),
    ] {
        records.push((u64::from(record), u64::try_from(bulk.len()).unwrap()));
        let tag: [u8; 3] = format!("{:03}", first + kind + 256)
            .as_bytes()
            .try_into()
            .unwrap();
        indexed_header(&mut bulk, tag, record);
        match record {
            1700 | 1710 | 1720 => {
                bulk.extend_from_slice(&[0; 10]);
                let uuid = if record == 1710 {
                    "00000000-0000-4000-8000-000000000100"
                } else {
                    "00000000-0000-4000-8000-000000000099"
                };
                lp_utf16(&mut bulk, &format!("TSpline.{uuid}.tsm"));
                push_marked_reference(&mut bulk, if record == 1710 { 1712 } else { 1702 });
            }
            1701 | 1711 => {
                for value in [
                    "00000000-0000-4000-8000-000000000001",
                    "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
                    "",
                    "PrismMaterial-001",
                ] {
                    lp_utf16(&mut bulk, value);
                }
                push_marked_reference(&mut bulk, 1400);
                if record == 1701 {
                    push_marked_reference(&mut bulk, 1703);
                    push_marked_reference(&mut bulk, 1702);
                    push_marked_reference(&mut bulk, 1704);
                } else {
                    bulk.push(1);
                    bulk.extend_from_slice(&22u64.to_le_bytes());
                    bulk.push(1);
                    bulk.extend_from_slice(&7u32.to_le_bytes());
                    lp_utf16(&mut bulk, literal);
                    bulk.push(0);
                    lp_ascii(&mut bulk, "CD57BC48-50EC-47DC-975A-FB6DEA72F4DA");
                    lp_utf16(&mut bulk, literal);
                    bulk.push(1);
                    lp_utf16(&mut bulk, "11111111-2222-3333-4444-555555555555");
                    lp_utf16(&mut bulk, literal);
                    push_marked_reference(&mut bulk, 1712);
                    push_marked_reference(&mut bulk, 9999);
                }
                indexed_header(&mut bulk, *b"258", record);
            }
            _ => bulk.extend_from_slice(&[0; 10]),
        }
    }
    let meta =
        crate::test_support::streams_fixture::design_metastream_with_records(&types, &records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
        &synthetic_geometry_smbh(),
        &[],
        &bulk,
        &meta,
        &[
            (
                "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000099.tsm",
                tsm,
            ),
            (
                "FusionAssetName[Active]/TSplines.BlobParts/TSpline.00000000-0000-4000-8000-000000000100.tsm",
                tsm,
            ),
        ],
    )
}

pub(crate) fn generated_form_tsm() -> Vec<u8> {
    br"#TS0200
degree 3
cap-type G1CAPS
star-smoothness 0
units 1 meters
end-conditions SUBD_CREASES
star-knot-rule NURCCS
f 0 0
e 0 1
e 2 1
e 4 1
e 6 1
v 0 NORTH
v 2 NORTH
v 4 NORTH
v 6 NORTH
l 2 6 1 0 0 0 0
l 7 3 0 3 -1 0 0
l 4 0 3 1 0 0 0
l 1 5 2 0 -1 0 0
l 6 2 5 2 0 0 0
l 3 7 4 1 -1 0 0
l 0 4 7 3 0 0 0
l 5 1 6 2 -1 0 0
ec 0 0
ec 1 0
ec 2 0
ec 3 0
0g 0 0 0 1
0g 1 0 0 1
0g 1 1 0 1
0g 0 1 0 1
"
    .to_vec()
}

fn f3d_with_smbh_and_instance_properties_and_design(
    smbh: &[u8],
    properties: &[Vec<u8>],
    design_bulk: &[u8],
    design_records: &[(u64, u64)],
) -> Vec<u8> {
    let design_metastream = generated_design_metastream(design_records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        properties,
        design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_instance_properties_and_design_with_metastream(
    smbh: &[u8],
    properties: &[Vec<u8>],
    design_bulk: &[u8],
    design_metastream: &[u8],
) -> Vec<u8> {
    f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
        smbh,
        properties,
        design_bulk,
        design_metastream,
        &[],
    )
}

fn f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
    smbh: &[u8],
    properties: &[Vec<u8>],
    design_bulk: &[u8],
    design_metastream: &[u8],
    extra_entries: &[(&str, &[u8])],
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
    if !smbh.is_empty() {
        zip.start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
            stored,
        )
        .unwrap();
        zip.write_all(smbh).unwrap();
    }
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
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(design_bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(design_metastream).unwrap();
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
    for (name, payload) in extra_entries {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Assemble a synthetic `.f3d` ZIP with a manifest, a BREP `.smbh`, a `.smb`
/// snapshot, and a few side entries, mirroring the spec's naming families.
pub(crate) fn synthetic_f3d(include_smbh: bool) -> Vec<u8> {
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

pub(crate) fn synthetic_multi_asset_f3d(include_design_brep: bool) -> Vec<u8> {
    multi_asset_f3d(include_design_brep, "")
}

/// [`synthetic_multi_asset_f3d`] with a model BREP, whose Design asset is a
/// board asset: its manifest stores the subtype `PCBAssetSubType`.
pub(crate) fn synthetic_board_asset_f3d() -> Vec<u8> {
    multi_asset_f3d(true, "PCBAssetSubType")
}

fn multi_asset_f3d(include_design_brep: bool, design_subtype: &str) -> Vec<u8> {
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
    zip.write_all(
        &crate::manifest::encode_design_asset_with_subtype(
            "DesignAsset",
            DESIGN_GUID,
            design_subtype,
        )
        .unwrap(),
    )
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchAnnotationCase {
    Valid,
    Ambiguous,
    UnknownOperand,
    MetadataCount,
}

pub(crate) fn f3d_with_generated_sketch_annotation(case: SketchAnnotationCase) -> Vec<u8> {
    let (mut bulk, mut records) = generated_design_sketch_dimension_bulkstream();
    let offset = |index| {
        usize::try_from(
            records
                .iter()
                .find(|(record, _)| *record == index)
                .unwrap()
                .1,
        )
        .unwrap()
    };
    let owner = offset(1203);
    let parameter = offset(1204);
    let companion = offset(1205);
    if case != SketchAnnotationCase::Valid {
        let second_point_v = offset(1302) + 97;
        bulk[second_point_v..second_point_v + 8].copy_from_slice(&1.0f64.to_le_bytes());
    }
    if matches!(
        case,
        SketchAnnotationCase::UnknownOperand | SketchAnnotationCase::MetadataCount
    ) {
        let mut replacement = crate::design::test_support::parameter_record(
            Some(1203),
            "1e6mm",
            "Linear Dimension-2",
            Some("mm"),
            "d2",
            100_000.0,
        );
        replacement[4..7].copy_from_slice(b"271");
        replacement[7..11].copy_from_slice(&1204u32.to_le_bytes());
        bulk[parameter..companion].copy_from_slice(&replacement);
        bulk[owner + 40..owner + 48].copy_from_slice(&100_000.0f64.to_le_bytes());
    }
    let mut frame = Vec::new();
    crate::test_support::indexed_header(&mut frame, *b"274", 1304);
    frame.extend_from_slice(&[0; 8]);
    frame.push(1);
    frame.extend_from_slice(&3u32.to_le_bytes());
    let first = if case == SketchAnnotationCase::UnknownOperand {
        9999
    } else {
        1300
    };
    for (index, role) in [(0u32, 1u32), (first, 2), (1302, 2)] {
        crate::test_support::push_marked_reference(&mut frame, index);
        frame.extend_from_slice(&role.to_le_bytes());
    }
    frame.push(1);
    frame.extend_from_slice(
        &(if case == SketchAnnotationCase::MetadataCount {
            2u32
        } else {
            0
        })
        .to_le_bytes(),
    );
    frame.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    crate::test_support::push_marked_reference(&mut frame, 1203);
    frame.extend_from_slice(&2u32.to_le_bytes());
    for index in [1302, 1300] {
        crate::test_support::push_marked_reference(&mut frame, index);
    }
    frame.push(0);
    let paired = frame.len();
    crate::test_support::indexed_header(&mut frame, *b"273", 1304);
    frame.extend_from_slice(&[0; 8]);
    crate::test_support::push_marked_reference(&mut frame, 277);
    frame.resize(paired + 59, 0);
    insert_frame_in_preceding_companion(&mut bulk, &mut records, &frame);
    let meta = generated_design_sketch_dimension_metastream(&records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        &synthetic_geometry_smbh(),
        &[],
        &bulk,
        &meta,
    )
}

fn insert_frame_in_preceding_companion(
    bulk: &mut Vec<u8>,
    records: &mut [(u64, u64)],
    inserted: &[u8],
) {
    let offset = |index| {
        usize::try_from(
            records
                .iter()
                .find(|(record, _)| *record == index)
                .unwrap()
                .1,
        )
        .unwrap()
    };
    let start = offset(1202) + 58;
    let owner = offset(1203);
    let parameter = offset(1204);
    let companion = offset(1205);
    let mut frame = inserted.to_vec();
    let new_owner = start + frame.len();
    let new_parameter = new_owner + parameter - owner;
    frame.extend_from_slice(&bulk[owner..companion]);
    let new_companion = start + frame.len();
    frame.extend_from_slice(&bulk[companion..]);
    bulk.truncate(start);
    bulk.extend_from_slice(&frame);
    for (record, offset) in records.iter_mut() {
        *offset = match *record {
            1204 => u64::try_from(new_parameter).unwrap(),
            1203 => u64::try_from(new_owner).unwrap(),
            _ if *offset >= u64::try_from(companion).unwrap() => {
                *offset - u64::try_from(companion).unwrap() + u64::try_from(new_companion).unwrap()
            }
            _ => *offset,
        };
    }
}

/// An archive whose preceding companion holds a two-point presentation frame.
/// The frame's class tags are the type-table ordinals of `primary` and `paired`,
/// and neither type lists an entity.
pub(crate) fn f3d_with_generated_sketch_presentation(
    primary: &'static str,
    paired: &'static str,
) -> Vec<u8> {
    let (mut bulk, mut records) = generated_design_sketch_dimension_bulkstream();
    let mut frame = Vec::new();
    crate::test_support::indexed_header(&mut frame, *b"273", 1304);
    frame.extend_from_slice(&[0; 8]);
    frame.push(1);
    frame.extend_from_slice(&2u32.to_le_bytes());
    for (index, role) in [(1300u32, 0u32), (1302, 1)] {
        crate::test_support::push_marked_reference(&mut frame, index);
        frame.extend_from_slice(&role.to_le_bytes());
    }
    frame.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    let pair = frame.len();
    crate::test_support::indexed_header(&mut frame, *b"274", 1304);
    frame.extend_from_slice(&[0; 8]);
    crate::test_support::push_marked_reference(&mut frame, 277);
    frame.resize(pair + 59, 0);
    insert_frame_in_preceding_companion(&mut bulk, &mut records, &frame);
    let mut types = generated_design_types(GeneratedDesignMetastreamVariant::SketchDimension);
    types.push((primary, "", 3, "Fusion", &[]));
    types.push((paired, "", 1, "Dimension", &[]));
    let meta = design_metastream_with_records(&types, &records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        &synthetic_geometry_smbh(),
        &[],
        &bulk,
        &meta,
    )
}

pub(crate) fn f3d_with_work_axis(
    smbh: &[u8],
    literal: &str,
    damaged: bool,
    properties: &[(&str, u64)],
    foreign: bool,
) -> Vec<u8> {
    use crate::test_support::{indexed_header, lp_ascii, lp_utf16, push_marked_reference};
    let (mut bulk, mut records) = generated_design_bulkstream();
    if smbh.is_empty() {
        let end = records.iter().find(|(id, _)| *id == 985).unwrap().1;
        bulk.drain(..usize::try_from(end).unwrap());
        records.retain(|(id, _)| *id != 899);
        for (_, offset) in &mut records {
            *offset = offset.checked_sub(end).unwrap();
        }
    }
    let mut types = generated_design_types(GeneratedDesignMetastreamVariant::Form);
    if smbh.is_empty() {
        for row in &mut types {
            if row.4.contains(&899) {
                row.4 = &[];
            }
        }
    }
    types.retain(|row| row.4 != [1500]);
    for row in &mut types {
        if row.4.contains(&1600) {
            row.4 = &[];
        }
    }
    records.push((1400, u64::try_from(bulk.len()).unwrap()));
    indexed_header(&mut bulk, *b"268", 1400);
    bulk.extend_from_slice(&[0; 10]);
    bulk.extend_from_slice(&2u32.to_le_bytes());
    push_marked_reference(&mut bulk, 1500);
    push_marked_reference(&mut bulk, 1700);
    bulk.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut bulk, "WorkAxis");
    bulk.extend_from_slice(&1u32.to_le_bytes());
    bulk.extend_from_slice(&[0; 78]);
    indexed_header(&mut bulk, *b"261", 1400);
    let carrier_tag: [u8; 3] = format!("{:03}", types.len() + 256)
        .as_bytes()
        .try_into()
        .unwrap();
    types.push((
        "F2A7590D-6654-4674-B393-A2AEF4FEC48A",
        "",
        4,
        "Fusion",
        &[1500],
    ));
    records.push((1500, u64::try_from(bulk.len()).unwrap()));
    indexed_header(&mut bulk, carrier_tag, 1500);
    bulk.extend_from_slice(&[0; 4]);
    lp_ascii(&mut bulk, literal);
    bulk.extend_from_slice(&[0, u8::from(!properties.is_empty())]);
    if !properties.is_empty() {
        bulk.extend_from_slice(&u32::try_from(properties.len()).unwrap().to_le_bytes());
        for (key, value) in properties {
            lp_ascii(&mut bulk, key);
            lp_ascii(&mut bulk, "IntrinsicMetaTypeuint64");
            bulk.extend_from_slice(&value.to_le_bytes());
        }
    }
    bulk.extend_from_slice(&8u32.to_le_bytes());
    for value in [0.0_f64, 0.0, 2.0, 0.0, 0.0, 1.0, -1.5, -1.0] {
        bulk.extend_from_slice(&value.to_le_bytes());
    }
    bulk.extend_from_slice(&19u32.to_le_bytes());
    bulk.push(0);
    for value in [if damaged { f64::NAN } else { -1.0 }, -1.0, -1.0] {
        bulk.extend_from_slice(&value.to_le_bytes());
    }
    bulk.extend_from_slice(&1u32.to_le_bytes());
    if foreign {
        let guid = "69EE2FA7-BCC7-449E-9CA9-976CEFDFED44";
        bulk.push(1);
        bulk.extend_from_slice(&1600u64.to_le_bytes());
        lp_ascii(&mut bulk, guid);
        bulk.push(1);
        bulk.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bulk, "11111111-2222-3333-4444-555555555555");
        bulk.push(0);
        lp_ascii(&mut bulk, guid);
        lp_utf16(&mut bulk, literal);
        bulk.push(1);
        lp_utf16(&mut bulk, "11111111-2222-3333-4444-555555555555");
        lp_utf16(&mut bulk, literal);
    } else {
        push_marked_reference(&mut bulk, 1600);
    }
    bulk.extend_from_slice(&[0, 0, 1, 0, 0, 0]);
    indexed_header(&mut bulk, *b"260", 1500);
    let owner_tag: [u8; 3] = format!("{:03}", types.len() + 256)
        .as_bytes()
        .try_into()
        .unwrap();
    let parameter_tag: [u8; 3] = format!("{:03}", types.len() + 257)
        .as_bytes()
        .try_into()
        .unwrap();
    let companion_tag: [u8; 3] = format!("{:03}", types.len() + 258)
        .as_bytes()
        .try_into()
        .unwrap();
    types.extend([
        (
            "00000000-0000-0000-0000-000000001700",
            "",
            1,
            "Dimension",
            &[1700][..],
        ),
        (
            "00000000-0000-0000-0000-000000001701",
            "",
            1,
            "Dimension",
            &[1701][..],
        ),
        (
            "00000000-0000-0000-0000-000000001702",
            "",
            1,
            "Dimension",
            &[1702][..],
        ),
    ]);
    records.push((1700, u64::try_from(bulk.len()).unwrap()));
    let mut owner = crate::design::test_support::parameter_owner_frame();
    owner[4..7].copy_from_slice(&owner_tag);
    owner[7..11].copy_from_slice(&1700u32.to_le_bytes());
    for at in [25, 68, 94] {
        owner[at..at + 4].copy_from_slice(&1400u32.to_le_bytes());
    }
    owner[35..39].copy_from_slice(&0u32.to_le_bytes());
    owner[49..53].copy_from_slice(&1701u32.to_le_bytes());
    owner[82..86].copy_from_slice(&1702u32.to_le_bytes());
    bulk.extend_from_slice(&owner);
    indexed_header(&mut bulk, owner_tag, 1700);
    records.push((1701, u64::try_from(bulk.len()).unwrap()));
    let mut parameter = crate::design::test_support::parameter_record(
        Some(1700),
        literal,
        "AlongDistance",
        Some("mm"),
        "Literal",
        1.25,
    );
    parameter[4..7].copy_from_slice(&parameter_tag);
    parameter[7..11].copy_from_slice(&1701u32.to_le_bytes());
    bulk.extend_from_slice(&parameter);
    records.push((1702, u64::try_from(bulk.len()).unwrap()));
    let mut companion = vec![0u8; 58];
    companion[0..4].copy_from_slice(&3u32.to_le_bytes());
    companion[4..7].copy_from_slice(&companion_tag);
    companion[7..11].copy_from_slice(&1702u32.to_le_bytes());
    companion[31] = 1;
    companion[32..36].copy_from_slice(&1700u32.to_le_bytes());
    companion[42..50].copy_from_slice(&1_700_000_000_000_000u64.to_le_bytes());
    bulk.extend_from_slice(&companion);
    let meta = design_metastream_with_records(&types, &records);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(smbh, &[], &bulk, &meta)
}
