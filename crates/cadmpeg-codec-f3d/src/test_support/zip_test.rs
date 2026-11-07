// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.f3d` ZIP archive builders.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use zip::CompressionMethod;

use crate::container;
use crate::test_support::act_stream_test::{generated_act_bulkstream, generated_act_metastream};
use crate::test_support::lost_edge_test::remove_body_lost_edge_bulkstream;
use crate::test_support::manifest_test::{
    write_synthetic_manifests, write_synthetic_manifests_with_version,
};
use crate::test_support::protein_test::{
    generated_definition_catalog_for, generated_instance_properties_for,
    generated_schema_from_paged,
};
use crate::test_support::sketch_stream_test::generated_design_bulkstream;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::smbh_header_test::synthetic_smbh;
use crate::test_support::smbh_revision_test::scrubbed_definition;
use crate::test_support::streams_test::{
    design_metastream_with_records_and_secondary, generated_design_base_feature_bulkstream,
    generated_design_base_feature_metastream, generated_design_base_flange_bulkstream,
    generated_design_base_flange_metastream, generated_design_canvas_bulkstream,
    generated_design_canvas_metastream, generated_design_copy_paste_bodies_bulkstream,
    generated_design_copy_paste_bodies_metastream, generated_design_copy_paste_bulkstream,
    generated_design_copy_paste_metastream, generated_design_form_bulkstream,
    generated_design_form_metastream, generated_design_metastream,
    generated_design_remove_body_bulkstream, generated_design_remove_body_metastream,
    generated_design_sketch_dimension_bulkstream, generated_design_sketch_dimension_metastream,
    generated_design_surface_stitch_bulkstream, generated_design_surface_stitch_metastream,
    generated_design_types, generated_flat_pattern_settings_bulkstream,
    generated_flat_pattern_settings_metastream, GeneratedDesignMetastreamVariant,
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
        .model
        .procedural_surfaces
        .first()
        .expect("revision surface construction");
    let expected = scrubbed_definition(procedural.definition());
    let kind = serde_json::to_value(procedural.definition()).expect("kind")["kind"]
        .as_str()
        .expect("kind string")
        .to_string();
    assert_eq!(kind, expected_kind);
    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less revision surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less revision surface round trip");
    let actual = scrubbed_definition(
        round_trip
            .ir()
            .model
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
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(smbh).unwrap();
    zip.start_file(name, stored).unwrap();
    zip.write_all(payload).unwrap();
    zip.finish().unwrap().into_inner()
}

/// An archive whose preview entry fails its integrity check: the stored
/// payload keeps one flipped byte, so the entry cannot be opened and is
/// given up as a `zip.entry` part while the document still decodes.
pub(crate) fn f3d_with_unreadable_preview(smbh: &[u8]) -> Vec<u8> {
    const NAME: &str = "FusionAssetName[Active]/Previews/thumbnail.png";
    let mut bytes = f3d_with_configuration(smbh, NAME, b"preview-image-payload");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, root).unwrap();
    let payload = usize::try_from(
        archive
            .entry(NAME)
            .unwrap()
            .data_start
            .expect("stored preview frame"),
    )
    .unwrap();
    drop(ctx);
    bytes[payload] ^= 1;
    bytes
}

/// An archive whose body entry fails its integrity check: the stored
/// body stream keeps one flipped byte under a valid container, so the
/// BREP framing fails and the entry is contained as a failed model
/// stream while the document still decodes.
pub(crate) fn f3d_with_unreadable_body_smbh_and_manifest_version(
    smbh: &[u8],
    version: &str,
) -> Vec<u8> {
    const NAME: &str = "FusionAssetName[Active]/Breps.BlobParts/Body1.smbh";
    let bytes = f3d_with_smbh_and_manifest_version(smbh, version);
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("synthetic archive opens");
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("synthetic entry opens");
        let name = entry.name().expect("synthetic entry name").into_owned();
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content).expect("synthetic entry reads");
        entries.push((name, content));
    }
    drop(archive);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    for (name, mut content) in entries {
        if name == NAME {
            content[0] ^= 1;
        }
        zip.start_file(name, stored)
            .expect("synthetic entry writes");
        zip.write_all(&content).expect("synthetic entry writes");
    }
    zip.finish().expect("synthetic archive closes").into_inner()
}

pub(crate) fn f3d_with_smbh_and_protein(smbh: &[u8]) -> Vec<u8> {
    f3d_with_smbh_and_protein_guids(smbh, &["11111111-2222-3333-4444-555555555555"])
}

/// An archive whose first protein asset keeps one flipped byte under a
/// valid container, so that asset framing fails and the entry is given
/// up as a `zip.entry` part while later assets still decode.
pub(crate) fn f3d_with_damaged_protein_asset(smbh: &[u8]) -> Vec<u8> {
    let bytes = f3d_with_smbh_and_protein_guids(
        smbh,
        &[
            "11111111-2222-3333-4444-555555555555",
            "22222222-3333-4444-5555-666666666666",
        ],
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("synthetic archive opens");
    let mut entries = Vec::new();
    let mut damaged = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("synthetic entry opens");
        let name = entry.name().expect("synthetic entry name").into_owned();
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content).expect("synthetic entry reads");
        if !damaged && name.ends_with(".protein") {
            content[0] ^= 1;
            damaged = true;
        }
        entries.push((name, content));
    }
    assert!(damaged, "synthetic archive carries a protein asset");
    drop(archive);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    for (name, content) in entries {
        zip.start_file(name, stored)
            .expect("synthetic entry writes");
        zip.write_all(&content).expect("synthetic entry writes");
    }
    zip.finish().expect("synthetic archive closes").into_inner()
}

/// An archive whose Design bulk stream keeps one flipped byte, so the primary
/// index does not resolve and each Design section gives up that stream while
/// the document still decodes.
pub(crate) fn f3d_with_damaged_design_bulkstream(smbh: &[u8]) -> Vec<u8> {
    let bytes = f3d_with_smbh_and_protein(smbh);
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("synthetic archive opens");
    let mut entries = Vec::new();
    let mut damaged = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("synthetic entry opens");
        let name = entry.name().expect("synthetic entry name").into_owned();
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content).expect("synthetic entry reads");
        if !damaged && name.ends_with("Design1/BulkStream.dat") {
            content[0] ^= 1;
            damaged = true;
        }
        entries.push((name, content));
    }
    assert!(damaged, "synthetic archive carries a Design bulk stream");
    drop(archive);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    for (name, content) in entries {
        zip.start_file(name, stored)
            .expect("synthetic entry writes");
        zip.write_all(&content).expect("synthetic entry writes");
    }
    zip.finish().expect("synthetic archive closes").into_inner()
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

pub(crate) fn indexed_fixture_metastream(bulk: &[u8], primary: &[u32], paired: &[u32]) -> Vec<u8> {
    let headers: Vec<_> = bulk
        .windows(11)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            if bytes[..4] != 3_u32.to_le_bytes() || !bytes[4..7].iter().all(u8::is_ascii_digit) {
                return None;
            }
            let tag = std::str::from_utf8(&bytes[4..7])
                .ok()?
                .parse::<usize>()
                .ok()?;
            let id = u32::from_le_bytes(bytes[7..11].try_into().ok()?);
            primary.contains(&id).then_some((offset, tag, id))
        })
        .collect();
    let mut records = Vec::new();
    let mut secondary = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &(offset, tag, id) in &headers {
        if seen.insert(id) {
            records.push((u64::from(id), u64::try_from(offset).unwrap(), tag));
        } else if paired.contains(&id)
            && !secondary.iter().any(|(entity, _)| *entity == u64::from(id))
        {
            secondary.push((u64::from(id), u64::try_from(offset).unwrap()));
        }
    }
    assert_eq!(records.len(), primary.len());
    assert_eq!(secondary.len(), paired.len());
    let highest = headers.iter().map(|(_, tag, _)| *tag).max().unwrap();
    let mut entities = vec![Vec::new(); highest - 255];
    for &(id, _, tag) in &records {
        entities[tag - 256].push(id);
    }
    let guids: Vec<_> = (256..=highest)
        .map(|tag| format!("00000000-0000-4000-8000-{tag:012X}"))
        .collect();
    let types: Vec<_> = guids
        .iter()
        .zip(&entities)
        .map(|(guid, ids)| (guid.as_str(), "", 1, "Fusion", ids.as_slice()))
        .collect();
    let records: Vec<_> = records
        .into_iter()
        .map(|(id, offset, _)| (id, offset))
        .collect();
    design_metastream_with_records_and_secondary(&types, &records, &secondary)
}

fn fixture_secondary_index(meta: Vec<u8>, bulk: &[u8], ids: &[u64]) -> Vec<u8> {
    let parsed = crate::metastream::parse(&meta, "Design/MetaStream.dat").unwrap();
    let mut secondary: Vec<_> = parsed
        .secondary_records
        .iter()
        .map(|row| (row.entity_id, row.bulk_offset))
        .collect();
    for (ordinal, row) in parsed
        .records
        .iter()
        .enumerate()
        .filter(|(_, row)| ids.contains(&row.entity_id))
    {
        if secondary.iter().any(|(id, _)| *id == row.entity_id) {
            continue;
        }
        let start = usize::try_from(row.bulk_offset).unwrap();
        let end = parsed.records.get(ordinal + 1).map_or(bulk.len(), |next| {
            usize::try_from(next.bulk_offset).unwrap()
        });
        let target = u32::try_from(row.entity_id).unwrap().to_le_bytes();
        let mut matches = bulk[start + 11..end]
            .windows(11)
            .enumerate()
            .filter(|(_, header)| {
                header[..4] == 3_u32.to_le_bytes()
                    && header[4..7].iter().all(u8::is_ascii_digit)
                    && header[7..11] == target
            })
            .map(|(offset, _)| start + 11 + offset);
        if let (Some(at), None) = (matches.next(), matches.next()) {
            secondary.push((row.entity_id, u64::try_from(at).unwrap()));
        }
    }
    secondary.sort_by_key(|(_, at)| *at);
    let entities: Vec<Vec<_>> = parsed
        .types
        .iter()
        .map(|ty| ty.entities.values().copied().collect())
        .collect();
    let types: Vec<_> = parsed
        .types
        .iter()
        .zip(&entities)
        .map(|(ty, ids)| {
            (
                ty.type_guid.as_str(),
                ty.base_type_guid.value().map_or("", |base| base.as_str()),
                ty.version,
                ty.module.as_str(),
                ids.as_slice(),
            )
        })
        .collect();
    let records: Vec<_> = parsed
        .records
        .iter()
        .map(|row| (row.entity_id, row.bulk_offset))
        .collect();
    design_metastream_with_records_and_secondary(&types, &records, &secondary)
}

pub(crate) fn original_design_fixture(bytes: Vec<u8>, remove_body: bool) -> Vec<u8> {
    use crate::test_support::streams_test::design_metastream_with_records;
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content).unwrap();
        entries.push((name, content));
    }
    let (act_bulk, act_records) = generated_act_bulkstream();
    for (name, content) in &mut entries {
        if name == "FusionAssetName[Active]/FusionACTSegmentType1/BulkStream.dat" {
            *content = act_bulk.clone();
        } else if name == "FusionAssetName[Active]/FusionACTSegmentType1/MetaStream.dat" {
            *content = generated_act_metastream(&act_records);
        }
    }
    let meta_index = entries
        .iter()
        .position(|(name, _)| name == "FusionAssetName[Active]/Design1/MetaStream.dat")
        .unwrap();
    let bulk_index = entries
        .iter()
        .position(|(name, _)| name == "FusionAssetName[Active]/Design1/BulkStream.dat")
        .unwrap();
    let (meta, header_offset, indexed_offsets, circle_offset, canvas_offsets) = {
        let parsed =
            crate::metastream::parse(&entries[meta_index].1, &entries[meta_index].0).unwrap();
        let offset = |id| {
            parsed
                .records
                .iter()
                .find(|row| row.entity_id == id)
                .unwrap()
                .bulk_offset
        };
        let header_offset = offset(9000);
        let indexed_offsets = [offset(123), offset(4645), offset(4646)];
        let entities: Vec<Vec<u64>> = parsed
            .types
            .iter()
            .map(|ty| {
                let mut ids: Vec<_> = ty.entities.values().copied().collect();
                if ty.type_guid.as_str() == "00000000-0000-4000-8000-000000000264" {
                    ids.push(600);
                    ids.sort_unstable();
                }
                ids.into_iter()
                    .filter(|id| match ty.type_guid.as_str() {
                        "00000000-0000-4000-8000-000000000264" => {
                            !(matches!(*id, 123 | 1701 | 1702 | 1703 | 4645 | 4646)
                                || remove_body && *id == 1700)
                        }
                        "33333333-4444-5555-6666-777777777777" => *id != 9000,
                        _ => true,
                    })
                    .collect()
            })
            .collect();
        let types: Vec<_> = parsed
            .types
            .iter()
            .zip(&entities)
            .filter(|(ty, _)| {
                ty.module != "Fixture"
                    && ty.type_guid.as_str() != "F0130424-8B7E-4092-93C9-1CA807482534"
            })
            .map(|(ty, ids)| {
                (
                    if ty.type_guid.as_str() == "00000000-0000-4000-8000-000000000264" {
                        "F0130424-8B7E-4092-93C9-1CA807482534"
                    } else {
                        ty.type_guid.as_str()
                    },
                    ty.base_type_guid.value().map_or("", |base| base.as_str()),
                    ty.version,
                    ty.module.as_str(),
                    ids.as_slice(),
                )
            })
            .collect();
        let records: Vec<_> = parsed
            .records
            .iter()
            .filter(|row| !matches!(row.entity_id, 9000 | 123 | 4645 | 4646))
            .map(|row| {
                (
                    row.entity_id,
                    if row.bulk_offset > header_offset {
                        row.bulk_offset - 11
                    } else {
                        row.bulk_offset
                    },
                )
            })
            .collect();
        (
            design_metastream_with_records(&types, &records),
            header_offset,
            indexed_offsets,
            offset(600),
            parsed
                .types
                .iter()
                .any(|ty| ty.type_guid.as_str() == "c0a00000-0000-4000-8000-000000000001")
                .then(|| (offset(10), offset(20), offset(30))),
        )
    };
    let bulk = &mut entries[bulk_index].1;
    if let Some((geometry, asset, scope)) = canvas_offsets {
        for (offset, newer, original) in [
            (geometry, b"281", b"273"),
            (geometry + 219, b"282", b"274"),
            (asset, b"283", b"275"),
            (scope, b"284", b"276"),
            (scope + 163, b"285", b"277"),
        ] {
            let at = usize::try_from(offset).unwrap() + 4;
            assert_eq!(&bulk[at..at + 3], newer);
            bulk[at..at + 3].copy_from_slice(original);
        }
    }
    let circle = usize::try_from(circle_offset).unwrap();
    assert_eq!(&bulk[circle + 4..circle + 7], b"280");
    bulk[circle + 4..circle + 7].copy_from_slice(b"264");
    for (ordinal, offset) in indexed_offsets.into_iter().enumerate() {
        let at = usize::try_from(offset).unwrap();
        assert_eq!(&bulk[at + 4..at + 7], b"264");
        bulk[at + 4..at + 7].copy_from_slice(if ordinal == 0 { b"322" } else { b"419" });
        if ordinal == 0 {
            bulk[at + 7..at + 11].fill(0);
        }
    }
    let at = usize::try_from(header_offset).unwrap();
    assert_eq!(&bulk[at + 4..at + 7], b"258");
    drop(bulk.drain(at..at + 11));
    entries[meta_index].1 = meta;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    for (name, content) in entries {
        zip.start_file(name, stored).unwrap();
        zip.write_all(&content).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn sketch_dimension_metastream(
    records: &[(u64, u64)],
    extra_types: &[(
        &'static str,
        &'static str,
        u32,
        &'static str,
        &'static [u64],
    )],
) -> Vec<u8> {
    let owner_length =
        u64::try_from(crate::design::test_support::parameter_owner_frame().len()).unwrap();
    let secondary: Vec<_> = records
        .iter()
        .filter(|(id, _)| matches!(id, 1200 | 1203))
        .map(|(id, offset)| (*id, offset + owner_length))
        .collect();
    if secondary.is_empty() && extra_types.is_empty() {
        return generated_design_sketch_dimension_metastream(records);
    }
    let mut types = generated_design_types(GeneratedDesignMetastreamVariant::SketchDimension);
    types.extend_from_slice(extra_types);
    design_metastream_with_records_and_secondary(&types, records, &secondary)
}

pub(crate) fn f3d_with_smbh_and_protein_with_original_sketch_dimension(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_sketch_dimension_bulkstream();
    let design_metastream = sketch_dimension_metastream(&design_records, &[]);
    let design_metastream =
        fixture_secondary_index(design_metastream, &design_bulk, &[584, 1100, 1400, 1600]);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_sketch_dimension(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (mut design_bulk, mut design_records) = generated_design_sketch_dimension_bulkstream();
    let mut recipe_header = Vec::new();
    crate::test_support::indexed_header(&mut recipe_header, *b"275", 1305);
    let recipe_offset = design_bulk
        .windows(recipe_header.len())
        .position(|window| window == recipe_header)
        .expect("synthetic recipe header");
    design_bulk[recipe_offset + 4..recipe_offset + 7].copy_from_slice(b"283");
    design_records.push((
        1305,
        u64::try_from(recipe_offset).expect("synthetic recipe offset"),
    ));
    design_records.sort_by_key(|(_, offset)| *offset);
    let design_metastream = sketch_dimension_metastream(
        &design_records,
        &[
            (
                "00000000-0000-0000-0000-000000001303",
                "",
                1,
                "Dimension",
                &[],
            ),
            (
                "00000000-0000-0000-0000-000000001304",
                "",
                1,
                "Dimension",
                &[1304],
            ),
            (
                "00000000-0000-0000-0000-000000001305",
                "",
                1,
                "Dimension",
                &[1305],
            ),
        ],
    );
    let design_metastream =
        fixture_secondary_index(design_metastream, &design_bulk, &[584, 1100, 1400, 1600]);
    let act = crate::test_support::act_stream_test::generated_complete_act_bulkstream();
    f3d_with_smbh_and_instance_properties_and_design_with_act(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
        &[],
        &act,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_canvas(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_canvas_bulkstream();
    let design_metastream = generated_design_canvas_metastream(&design_records);
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
    f3d_with_smbh_and_instance_properties_and_design_with_metastream_and_entries(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
        &[(
            "FusionAssetName[Active]/Design1/Images.BlobParts/a.png",
            b"PNG".as_slice(),
        )],
    )
}

/// A BREP archive with one unreferenced `FlatPattern` sheet-metal settings
/// frame.
pub(crate) fn f3d_with_smbh_and_flat_pattern_settings(smbh: &[u8]) -> Vec<u8> {
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let (settings_bulk, settings_records) = generated_flat_pattern_settings_bulkstream();
    let settings_metastream = generated_flat_pattern_settings_metastream(&settings_records);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(
        "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
        stored,
    )
    .unwrap();
    zip.write_all(smbh).unwrap();
    zip.start_file("FlatPatternAssetSubType/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&settings_bulk).unwrap();
    zip.start_file("FlatPatternAssetSubType/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&settings_metastream).unwrap();
    zip.finish().unwrap().into_inner()
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_base_feature(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_base_feature_bulkstream();
    let design_metastream = generated_design_base_feature_metastream(&design_records);
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
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
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &design_bulk,
        &design_metastream,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_extrude(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (mut bulk, mut records) = generated_design_base_flange_bulkstream();
    let scope_start =
        usize::try_from(records.iter().find(|(id, _)| *id == 1400).unwrap().1).unwrap();
    let mut scope = Vec::new();
    crate::test_support::indexed_header(&mut scope, *b"268", 1400);
    scope.resize(208, 0);
    scope[21..25].copy_from_slice(&4_u32.to_le_bytes());
    scope[25..29].copy_from_slice(&2_u32.to_le_bytes());
    scope[30..34].copy_from_slice(&1_u32.to_le_bytes());
    scope.extend_from_slice(&3_u32.to_le_bytes());
    for reference in [2000_u32, 1501, 2200] {
        scope.push(1);
        scope.extend_from_slice(&reference.to_le_bytes());
        scope.extend_from_slice(&[0; 6]);
    }
    scope.extend_from_slice(&7_u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut scope, "Extrude");
    let mut tail = [0_u8; 78];
    tail[..4].copy_from_slice(&1_u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2_u32.to_le_bytes());
    scope.extend_from_slice(&tail);
    crate::test_support::indexed_header(&mut scope, *b"261", 1400);
    let mut closing = Vec::new();
    crate::test_support::indexed_header(&mut closing, *b"261", 1400);
    let scope_end = scope_start
        + bulk[scope_start..]
            .windows(closing.len())
            .position(|window| window == closing)
            .unwrap()
        + closing.len();
    bulk.splice(scope_start..scope_end, scope);

    for (id, class) in [(2000_u32, *b"264"), (1501, *b"377")] {
        let mut header = Vec::new();
        crate::test_support::indexed_header(&mut header, class, id);
        let offset = bulk
            .windows(header.len())
            .position(|window| window == header)
            .unwrap();
        if id == 1501 {
            bulk[offset + 4..offset + 7].copy_from_slice(b"264");
            let old: Vec<u8> = "800".encode_utf16().flat_map(u16::to_le_bytes).collect();
            let new: Vec<u8> = "277".encode_utf16().flat_map(u16::to_le_bytes).collect();
            let suffix = offset
                + bulk[offset..]
                    .windows(old.len())
                    .position(|window| window == old)
                    .unwrap();
            bulk[suffix..suffix + old.len()].copy_from_slice(&new);
        }
        records.push((u64::from(id), u64::try_from(offset).unwrap()));
    }
    let (sketch, sketch_records) =
        crate::test_support::sketch_stream_test::generated_design_sketch_bulkstream();
    let sketch_start =
        usize::try_from(sketch_records.iter().find(|(id, _)| *id == 1100).unwrap().1).unwrap();
    let sketch_offset = bulk.len();
    bulk.extend_from_slice(&sketch[sketch_start..]);
    records.extend(
        sketch_records
            .into_iter()
            .filter(|(id, _)| matches!(id, 1100 | 584))
            .map(|(id, offset)| {
                (
                    id,
                    u64::try_from(sketch_offset).unwrap() + offset
                        - u64::try_from(sketch_start).unwrap(),
                )
            }),
    );

    let owner_offset = u64::try_from(bulk.len()).unwrap();
    let mut owner = crate::design::test_support::parameter_owner_frame();
    owner[4..7].copy_from_slice(b"270");
    owner[7..11].copy_from_slice(&2200_u32.to_le_bytes());
    owner[25..29].copy_from_slice(&1400_u32.to_le_bytes());
    owner[40..48].copy_from_slice(&1.0_f64.to_le_bytes());
    owner[49..53].copy_from_slice(&2201_u32.to_le_bytes());
    owner[68..72].copy_from_slice(&1400_u32.to_le_bytes());
    owner[82..86].copy_from_slice(&2202_u32.to_le_bytes());
    owner[94..98].copy_from_slice(&1400_u32.to_le_bytes());
    let owner_end = owner_offset + u64::try_from(owner.len()).unwrap();
    bulk.extend_from_slice(&owner);
    crate::test_support::indexed_header(&mut bulk, *b"270", 2200);
    records.push((2200, owner_offset));
    let parameter_offset = u64::try_from(bulk.len()).unwrap();
    let mut parameter = crate::design::test_support::parameter_record(
        Some(2200),
        "10 mm",
        "AlongDistance",
        Some("mm"),
        "Depth",
        1.0,
    );
    parameter[4..7].copy_from_slice(b"271");
    parameter[7..11].copy_from_slice(&2201_u32.to_le_bytes());
    bulk.extend_from_slice(&parameter);
    records.push((2201, parameter_offset));
    records.push((2202, u64::try_from(bulk.len()).unwrap()));
    let mut companion = vec![0_u8; 58];
    companion[..4].copy_from_slice(&3_u32.to_le_bytes());
    companion[4..7].copy_from_slice(b"272");
    companion[7..11].copy_from_slice(&2202_u32.to_le_bytes());
    companion[31] = 1;
    companion[32..36].copy_from_slice(&2200_u32.to_le_bytes());
    companion[42..50].copy_from_slice(&1_700_000_000_000_000_u64.to_le_bytes());
    bulk.extend_from_slice(&companion);
    let mut types = generated_design_types(GeneratedDesignMetastreamVariant::BaseFlange);
    let base = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    types[8].4 = &[123, 1501, 2000, 4645, 4646];
    types[12].4 = &[1100, 1400];
    types[13] = (
        "00000000-0000-0000-0000-000000000584",
        base,
        1,
        "MSketch",
        &[584],
    );
    types[14] = (
        "00000000-0000-0000-0000-000000002200",
        base,
        1,
        "Dimension",
        &[2200],
    );
    types[15] = (
        "00000000-0000-0000-0000-000000002201",
        base,
        1,
        "Dimension",
        &[2201],
    );
    types[16] = (
        "00000000-0000-0000-0000-000000002202",
        base,
        1,
        "Dimension",
        &[2202],
    );
    let timeline_tag = u32::try_from(256 + types.len()).unwrap();
    records.push((2300, u64::try_from(bulk.len()).unwrap()));
    crate::test_support::lp_ascii(&mut bulk, &timeline_tag.to_string());
    bulk.extend_from_slice(&2300_u64.to_le_bytes());
    crate::test_support::lp_ascii(&mut bulk, "Timeline");
    bulk.extend_from_slice(&[0, 0, 1]);
    bulk.extend_from_slice(&277_u64.to_le_bytes());
    bulk.extend_from_slice(&[0, 0]);
    bulk.extend_from_slice(&2_u32.to_le_bytes());
    for id in [1100_u64, 1400] {
        bulk.push(1);
        bulk.extend_from_slice(&id.to_le_bytes());
        bulk.extend_from_slice(&[0, 0]);
    }
    types.push((
        crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
        crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID,
        2,
        "Fusion",
        &[2300],
    ));
    records.sort_by_key(|(_, offset)| *offset);
    let meta = design_metastream_with_records_and_secondary(&types, &records, &[(2200, owner_end)]);
    let meta = fixture_secondary_index(meta, &bulk, &[10, 30, 584, 1100, 1400, 1501, 2000]);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        &properties,
        &bulk,
        &meta,
    )
}

pub(crate) fn f3d_with_smbh_and_protein_with_generated_remove_body(smbh: &[u8]) -> Vec<u8> {
    let properties = vec![generated_instance_properties_for(
        "11111111-2222-3333-4444-555555555555",
    )];
    let (design_bulk, design_records) = generated_design_remove_body_bulkstream();
    let design_metastream = generated_design_remove_body_metastream(&design_records);
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
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
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
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
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
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
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
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
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
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
    let design_metastream = fixture_secondary_index(
        design_metastream,
        &design_bulk,
        &[10, 30, 584, 1100, 1400, 1500, 1600, 1700],
    );
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
    let design_metastream =
        fixture_secondary_index(design_metastream, design_bulk, &[584, 1100, 1400, 1600]);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        smbh,
        properties,
        design_bulk,
        &design_metastream,
    )
}

fn f3d_with_smbh_and_instance_properties_and_design_with_metastream(
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
    let act = crate::test_support::act_stream_test::generated_single_entity_act_bulkstream();
    f3d_with_smbh_and_instance_properties_and_design_with_act(
        smbh,
        properties,
        design_bulk,
        design_metastream,
        extra_entries,
        &act,
    )
}

fn f3d_with_smbh_and_instance_properties_and_design_with_act(
    smbh: &[u8],
    properties: &[Vec<u8>],
    design_bulk: &[u8],
    design_metastream: &[u8],
    extra_entries: &[(&str, &[u8])],
    act: &(Vec<u8>, Vec<(u64, u64)>),
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
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(design_bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(design_metastream).unwrap();
    let (act_bulk, act_records) = act;
    zip.start_file(
        "FusionAssetName[Active]/FusionACTSegmentType1/BulkStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(act_bulk).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/FusionACTSegmentType1/MetaStream.dat",
        stored,
    )
    .unwrap();
    zip.write_all(&generated_act_metastream(act_records))
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SketchAnnotationCase {
    Valid,
    Ambiguous,
    UnknownOperand,
    MetadataCount,
    DamagedSibling,
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
    if !matches!(
        case,
        SketchAnnotationCase::Valid | SketchAnnotationCase::DamagedSibling
    ) {
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
    crate::test_support::indexed_header(&mut frame, *b"281", 1304);
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
    let metadata_start = frame.len();
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
    crate::test_support::indexed_header(&mut frame, *b"282", 1304);
    frame.extend_from_slice(&[0; 8]);
    crate::test_support::push_marked_reference(&mut frame, 277);
    frame.resize(paired + 59, 0);
    let insertion = records.iter().find(|(id, _)| *id == 1202).unwrap().1 + 58;
    let entities: &[u64] = if case == SketchAnnotationCase::DamagedSibling {
        let mut damaged = frame.clone();
        let id_offset = 3u32.to_le_bytes().len() + b"281".len();
        damaged[id_offset..id_offset + 4].copy_from_slice(&1305u32.to_le_bytes());
        damaged[paired + id_offset..paired + id_offset + 4].copy_from_slice(&1305u32.to_le_bytes());
        damaged[metadata_start + 1..metadata_start + 5].copy_from_slice(&2u32.to_le_bytes());
        let valid_start = insertion + u64::try_from(damaged.len()).unwrap();
        damaged.extend_from_slice(&frame);
        insert_frame_in_preceding_companion(&mut bulk, &mut records, &damaged);
        records.push((1305, insertion));
        records.push((1304, valid_start));
        &[1305, 1304]
    } else {
        insert_frame_in_preceding_companion(&mut bulk, &mut records, &frame);
        records.push((1304, insertion));
        &[1304]
    };
    records.sort_by_key(|(_, offset)| *offset);
    let meta = sketch_dimension_metastream(
        &records,
        &[
            (
                "00000000-0000-4000-8000-000000000273",
                "",
                3,
                "Fusion",
                entities,
            ),
            (
                "00000000-0000-4000-8000-000000000274",
                "",
                1,
                "Dimension",
                &[],
            ),
        ],
    );
    let secondary: &[u64] = if case == SketchAnnotationCase::DamagedSibling {
        &[584, 1100, 1304, 1305, 1400, 1600]
    } else {
        &[584, 1100, 1304, 1400, 1600]
    };
    let meta = fixture_secondary_index(meta, &bulk, secondary);
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

/// An archive with an indexed two-point presentation frame.
/// Its class tags are the type-table ordinals of `primary` and `paired`.
pub(crate) fn f3d_with_generated_sketch_presentation(
    primary: &'static str,
    paired: &'static str,
) -> Vec<u8> {
    sketch_presentation_archive(primary, paired, false)
}

pub(crate) fn f3d_with_damaged_sketch_presentation_sibling(
    primary: &'static str,
    paired: &'static str,
) -> Vec<u8> {
    sketch_presentation_archive(primary, paired, true)
}

fn sketch_presentation_archive(
    primary: &'static str,
    paired: &'static str,
    damaged_sibling: bool,
) -> Vec<u8> {
    let (mut bulk, mut records) = generated_design_sketch_dimension_bulkstream();
    let mut inserted = Vec::new();
    let mut inserted_offsets = Vec::new();
    let entities: &[u64] = if damaged_sibling {
        &[1305, 1304]
    } else {
        &[1304]
    };
    for &entity in entities {
        let start = inserted.len();
        let mut frame = Vec::new();
        crate::test_support::indexed_header(&mut frame, *b"281", u32::try_from(entity).unwrap());
        frame.extend_from_slice(&[0; 8]);
        frame.push(1);
        frame.extend_from_slice(&2u32.to_le_bytes());
        for (index, role) in [(1300u32, 0u32), (1302, 1)] {
            let marker = frame.len();
            crate::test_support::push_marked_reference(&mut frame, index);
            if entity == 1305 && index == 1302 {
                frame[marker] = 0;
            }
            frame.extend_from_slice(&role.to_le_bytes());
        }
        frame.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        let pair = frame.len();
        crate::test_support::indexed_header(&mut frame, *b"282", u32::try_from(entity).unwrap());
        frame.extend_from_slice(&[0; 8]);
        crate::test_support::push_marked_reference(&mut frame, 277);
        frame.resize(pair + 59, 0);
        inserted_offsets.push((entity, start));
        inserted.extend_from_slice(&frame);
    }
    let insertion = records.iter().find(|(id, _)| *id == 1203).unwrap().1;
    let insertion_at = usize::try_from(insertion).unwrap();
    let frame_len = u64::try_from(inserted.len()).unwrap();
    for (_, offset) in &mut records {
        if *offset >= insertion {
            *offset += frame_len;
        }
    }
    bulk.splice(insertion_at..insertion_at, inserted);
    records.extend(
        inserted_offsets
            .into_iter()
            .map(|(entity, offset)| (entity, insertion + u64::try_from(offset).unwrap())),
    );
    records.sort_by_key(|(_, offset)| *offset);
    let meta = sketch_dimension_metastream(
        &records,
        &[
            (primary, "", 3, "Fusion", entities),
            (paired, "", 1, "Dimension", &[]),
        ],
    );
    let secondary: &[u64] = if damaged_sibling {
        &[584, 1100, 1304, 1305, 1400, 1600]
    } else {
        &[584, 1100, 1304, 1400, 1600]
    };
    let meta = fixture_secondary_index(meta, &bulk, secondary);
    f3d_with_smbh_and_instance_properties_and_design_with_metastream(
        &synthetic_geometry_smbh(),
        &[],
        &bulk,
        &meta,
    )
}
