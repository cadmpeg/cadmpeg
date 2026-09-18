// SPDX-License-Identifier: Apache-2.0

use crate::chunks::ArchiveVersion;
use crate::chunks::BoundedReader;
use crate::loss::Diagnostics;
use crate::settings;
use crate::test_support::test_dump::class_wrapper;
use crate::test_support::test_dump::crc_chunk_excluding;
use crate::test_support::test_dump::minimal_document;
use crate::test_support::test_dump::nested_crc_chunk;
use crate::test_support::test_dump::point_payload;
use crate::test_support::test_dump::short_chunk;
use crate::test_support::test_dump::table;
use crate::test_support::test_dump::tagged_attributes;
use crate::test_support::test_dump::units_record;
use crate::test_support::test_dump::POINT_CLASS;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use std::io::Cursor;

fn anonymous(
    archive: ArchiveVersion,
    minor: i32,
    body: &[u8],
    children: &[std::ops::Range<usize>],
    corrupt: bool,
) -> Vec<u8> {
    let prefix = [1_i32.to_le_bytes(), minor.to_le_bytes()].concat();
    let child_ranges: Vec<_> = children.iter().map(|r| r.start + 8..r.end + 8).collect();
    let mut chunk = crc_chunk_excluding(
        archive,
        0x4000_8000,
        &[prefix, body.to_vec()].concat(),
        &child_ranges,
    );
    if corrupt {
        let last = chunk.len() - 1;
        chunk[last] ^= 1;
    }
    chunk
}

fn rendering(archive: ArchiveVersion, object: bool, corrupt: &str) -> Vec<u8> {
    let mut channel_body = 7_i32.to_le_bytes().to_vec();
    channel_body.extend([0x33; 16]);
    channel_body.extend(
        (0..16)
            .map(|i| if i % 5 == 0 { 1.0 } else { 0.0 })
            .flat_map(f64::to_le_bytes),
    );
    let obsolete = anonymous(archive, 1, &channel_body, &[], corrupt == "obsolete");
    let mut material_body = [vec![0x11; 16], vec![0x22; 16], 1_i32.to_le_bytes().to_vec()].concat();
    let obsolete_start = material_body.len();
    material_body.extend(obsolete);
    let obsolete_end = material_body.len();
    material_body.extend([0x44; 16]);
    material_body.extend(0_i32.to_le_bytes());
    let material = anonymous(
        archive,
        1,
        &material_body,
        std::slice::from_ref(&(obsolete_start..obsolete_end)),
        corrupt == "material",
    );
    let mut body = 1_i32.to_le_bytes().to_vec();
    let material_start = body.len();
    body.extend(material);
    let mut children = Vec::new();
    children.push(material_start..body.len());
    if object {
        let channel = anonymous(archive, 1, &channel_body, &[], corrupt == "channel");
        let mut mapping_body = [vec![0x11; 16], 1_i32.to_le_bytes().to_vec()].concat();
        let channel_start = mapping_body.len();
        mapping_body.extend(channel);
        let mapping = anonymous(
            archive,
            0,
            &mapping_body,
            std::slice::from_ref(&(channel_start..mapping_body.len())),
            corrupt == "mapping",
        );
        body.extend(1_i32.to_le_bytes());
        let mapping_start = body.len();
        body.extend(mapping);
        children.push(mapping_start..body.len());
        body.extend([1, 1, 0]);
    }
    anonymous(
        archive,
        if object { 3 } else { 0 },
        &body,
        &children,
        corrupt == "outer",
    )
}

#[test]
fn rendering_checksums_follow_each_nested_chunk_owner() {
    for archive in [ArchiveVersion::V5, ArchiveVersion::V8] {
        for object in [false, true] {
            for corrupt in [
                "none", "obsolete", "material", "outer", "channel", "mapping",
            ] {
                if !object && matches!(corrupt, "channel" | "mapping") {
                    continue;
                }
                let bytes = rendering(archive, object, corrupt);
                let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
                let mut warnings = Diagnostics::new();
                let kind = if object {
                    settings::RenderingAttributesKind::Object
                } else {
                    settings::RenderingAttributesKind::Layer
                };
                settings::parse_rendering_attributes(
                    &bytes,
                    &mut reader,
                    archive,
                    kind,
                    &mut warnings,
                )
                .unwrap();
                assert_eq!(reader.remaining(), 0);
                assert_eq!(
                    warnings.len(),
                    usize::from(corrupt != "none"),
                    "{archive:?}/{object}/{corrupt}: {warnings:?}"
                );
                for warning in warnings {
                    assert_eq!(
                        warning.code,
                        Some(crate::loss::RhinoLossCode::IntegrityFailure)
                    );
                }
            }
        }
    }
}

#[test]
fn complete_object_rendering_reports_nested_crc_without_losing_geometry_or_source() {
    for (archive, version) in [(ArchiveVersion::V5, "50"), (ArchiveVersion::V8, "80")] {
        for corrupt in [
            "none", "obsolete", "material", "outer", "channel", "mapping",
        ] {
            let rendering = rendering(archive, true, corrupt);
            let attributes_body = tagged_attributes(&[(5, rendering.clone())], 0);
            // Packed version, UUID, layer index and rendering item tag precede the child.
            let rendering_start = 1 + 16 + 4 + 1;
            let attributes = crc_chunk_excluding(
                archive,
                0x0200_8072,
                &attributes_body,
                std::slice::from_ref(&(rendering_start..rendering_start + rendering.len())),
            );
            let object_body = [
                short_chunk(archive, 0x8200_0071, 1),
                class_wrapper(archive, POINT_CLASS, &point_payload([1.0, 2.0, 3.0])),
                attributes,
                short_chunk(archive, 0x8200_007f, 0),
            ]
            .concat();
            let record = nested_crc_chunk(archive, 0x2000_8070, &object_body);
            let source = minimal_document(
                version,
                &[
                    table(archive, 0x1000_0014, &[]),
                    table(archive, 0x1000_0015, &[units_record(archive, 2)]),
                    table(archive, 0x1000_0013, std::slice::from_ref(&record)),
                ],
            );
            let decoded = crate::RhinoCodec
                .decode(&mut Cursor::new(source), &DecodeOptions::default())
                .unwrap();
            let losses: Vec<_> = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind())
                .collect();
            assert_eq!(
                losses.len(),
                usize::from(corrupt != "none"),
                "{archive:?}/{corrupt}: {losses:?}"
            );
            let reread = CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
            for ir in [decoded.ir(), &reread] {
                assert_eq!(ir.model.points.len(), 1, "{archive:?}/{corrupt}");
                assert_eq!(
                    ir.model.points[0].position,
                    cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                );
            }
            assert!(decoded
                .source_fidelity()
                .retained_records()
                .values()
                .any(|source| source.data() == Some(record.as_slice())));
        }
    }
}
