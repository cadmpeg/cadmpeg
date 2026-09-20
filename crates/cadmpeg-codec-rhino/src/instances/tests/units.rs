// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::EditableDecodeResult;

use crate::chunks::ArchiveVersion;
use crate::loss::RhinoLossCode;
use crate::test_support::test_dump as bytes;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use std::io::Cursor;

fn definition_record(
    archive: ArchiveVersion,
    id: [u8; 16],
    unit: u32,
    scale: f64,
    legacy_unit: u32,
    legacy_scale: f64,
) -> Vec<u8> {
    let units = bytes::unit_detail(archive, unit, scale);
    let mut bounds = Vec::new();
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
        bounds.extend(value.to_le_bytes());
    }
    let mut class_data = if archive == ArchiveVersion::V5 {
        let mut prefix = vec![0x16];
        prefix.extend(id);
        prefix.extend(0_i32.to_le_bytes()); // empty member list
        prefix.extend(bytes::utf16_bytes("unit proof"));
        for _ in 0..3 {
            prefix.extend(bytes::utf16_bytes(""));
        }
        prefix.extend(bounds);
        prefix.extend(0_u32.to_le_bytes()); // static definition
        prefix.extend(bytes::utf16_bytes("")); // no linked path
        prefix.extend([0_u8; 48]); // empty legacy file checksum
        prefix.extend(legacy_unit.to_le_bytes());
        prefix.extend(legacy_scale.to_le_bytes());
        prefix.push(0); // legacy relative-path flag
        let tail = [0_u8; 8]; // linked depth and appearance
        let direct = [prefix.as_slice(), &tail].concat();
        let mut payload = prefix;
        payload.extend(units);
        payload.extend(tail);
        payload.extend(crc32fast::hash(&direct).to_le_bytes());
        payload
    } else {
        let component =
            bytes::model_component_attributes(archive, id, i32::from(id[0]), "unit proof");
        let versions = [1_i32.to_le_bytes(), 0_i32.to_le_bytes()].concat();
        let kind = 1_u32.to_le_bytes();
        let mut tail = Vec::new();
        for _ in 0..3 {
            tail.extend(bytes::utf16_bytes(""));
        }
        tail.extend(bounds);
        tail.extend([0, 0]); // no member list or linked payload
        let direct = [versions.as_slice(), &kind, &tail].concat();
        let mut payload = [versions, component, kind.to_vec(), units, tail].concat();
        payload.extend(crc32fast::hash(&direct).to_le_bytes());
        let mut class_data = bytes::long_chunk(archive, 0x4000_8000, &payload);
        class_data.extend(0_u32.to_le_bytes()); // child-only class data
        class_data
    };
    class_data = bytes::long_chunk(archive, 0x0002_fffc, &class_data);
    let class = [
        bytes::crc_chunk(archive, 0x0002_fffb, &bytes::INSTANCE_DEFINITION_CLASS),
        class_data,
        bytes::short_chunk(archive, 0x8002_7fff, 0),
    ]
    .concat();
    let mut record = bytes::long_chunk(archive, 0x0002_7ffa, &class);
    record.extend(0_u32.to_le_bytes()); // child-only definition record
    bytes::long_chunk(archive, 0x2000_8076, &record)
}

#[test]
fn unit_scale_admission_preserves_redundant_bits_and_rejects_invalid_custom_scales() {
    for archive in [
        ArchiveVersion::V5,
        ArchiveVersion::V6,
        ArchiveVersion::V7,
        ArchiveVersion::V8,
    ] {
        for unit in [0, 2, 11, 255, u32::MAX] {
            for (scale, custom_valid) in [
                (0.001, true),
                (1.0, true),
                (1.0e306, true),
                (f64::MIN_POSITIVE, true),
                (0.0, false),
                (-1.0, false),
                (f64::INFINITY, false),
                (f64::NEG_INFINITY, false),
                (f64::from_bits(0x7ff8_1234_5678_9abc), false),
                (1.234_321_012_343_21e308, false),
                (f64::MAX, false),
            ] {
                let admitted = unit != 11 || custom_valid;
                let repaired = (unit == 0 && scale != 1.0) || (unit == 2 && scale != 0.001);
                let record = definition_record(archive, [0x51; 16], unit, scale, 2, 0.001);
                let later = definition_record(archive, [0x52; 16], 2, 0.001, 2, 0.001);
                let document = bytes::document_with_definitions(
                    &archive.value().to_string(),
                    archive,
                    &[record.clone(), later],
                    &[],
                );
                let scan = crate::container::scan_owned(document.clone()).unwrap();
                assert_eq!(
                    scan.definitions.definitions.len(),
                    1 + usize::from(admitted)
                );
                let source_offset = scan
                    .tables
                    .iter()
                    .find(|table| table.typecode == 0x1000_0021)
                    .unwrap()
                    .records[0]
                    .range
                    .start as u64;
                if admitted {
                    let units = &scan.definitions.definitions[0].units;
                    assert_eq!(units.unit, unit);
                    assert_eq!(units.meters_per_unit_bits, scale.to_bits());
                }
                for container_only in [false, true] {
                    let result = EditableDecodeResult::from(
                        crate::RhinoCodec
                            .decode(
                                &mut Cursor::new(&document),
                                &DecodeOptions {
                                    container_only,
                                    ..DecodeOptions::default()
                                },
                            )
                            .unwrap(),
                    );
                    for (code, count) in [
                        (RhinoLossCode::IntegrityFailure, 0),
                        (
                            RhinoLossCode::ContainerInstanceDefinitionDegraded,
                            usize::from(!admitted),
                        ),
                        (RhinoLossCode::RedundantFieldRepaired, usize::from(repaired)),
                    ] {
                        let losses: Vec<_> = result
                            .report()
                            .losses
                            .iter()
                            .filter(|loss| loss.code == code.kind())
                            .collect();
                        assert_eq!(losses.len(), count, "{archive:?} unit={unit} scale={scale} container_only={container_only}: {losses:?}");
                        for loss in losses {
                            assert_eq!(loss.provenance.as_ref().unwrap().offset, source_offset);
                        }
                    }
                    let reread =
                        CadIr::from_json(&serde_json::to_string(result.ir()).unwrap()).unwrap();
                    if !container_only {
                        for ir in [result.ir(), &reread] {
                            let native =
                                serde_json::to_value(ir.native.namespace("rhino").unwrap())
                                    .unwrap();
                            let definitions = native["product_definitions"].as_array().unwrap();
                            assert_eq!(definitions.len(), 1 + usize::from(admitted));
                            if admitted {
                                let first = &definitions[0];
                                assert_eq!(first["unit_system"], unit);
                                if scale.is_finite() {
                                    assert_eq!(
                                        first["meters_per_unit"].as_f64().unwrap().to_bits(),
                                        scale.to_bits()
                                    );
                                    assert!(first.get("meters_per_unit_bits").is_none());
                                } else {
                                    assert_eq!(
                                        first["meters_per_unit_bits"].as_u64(),
                                        Some(scale.to_bits())
                                    );
                                    assert!(first.get("meters_per_unit").is_none());
                                }
                            } else {
                                assert!(result
                                    .source_fidelity()
                                    .retained_records()
                                    .values()
                                    .any(|source| source.data() == Some(record.as_slice())));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn legacy_units_are_replaced_by_the_complete_unit_detail() {
    for legacy_unit in [2, 11, 255, u32::MAX] {
        for legacy_scale in [0.001, 0.0, -1.0, f64::INFINITY, f64::NAN] {
            let record = definition_record(
                ArchiveVersion::V5,
                [0x51; 16],
                2,
                0.001,
                legacy_unit,
                legacy_scale,
            );
            let document =
                bytes::document_with_definitions("50", ArchiveVersion::V5, &[record], &[]);
            for container_only in [false, true] {
                let result = crate::RhinoCodec
                    .decode(
                        &mut Cursor::new(&document),
                        &DecodeOptions {
                            container_only,
                            ..DecodeOptions::default()
                        },
                    )
                    .unwrap();
                assert!(
                    result.report().losses.iter().all(|loss| ![
                        RhinoLossCode::ContainerInstanceDefinitionDegraded.kind(),
                        RhinoLossCode::IntegrityFailure.kind(),
                        RhinoLossCode::RedundantFieldRepaired.kind(),
                    ]
                    .contains(&loss.code)),
                    "{:?}",
                    result.report().losses
                );
                let reread =
                    CadIr::from_json(&serde_json::to_string(result.ir()).unwrap()).unwrap();
                if !container_only {
                    let native =
                        serde_json::to_value(reread.native.namespace("rhino").unwrap()).unwrap();
                    assert_eq!(native["product_definitions"].as_array().unwrap().len(), 1);
                    assert_eq!(native["product_definitions"][0]["unit_system"], 2);
                    assert_eq!(native["product_definitions"][0]["meters_per_unit"], 0.001);
                }
            }
        }
    }
}
