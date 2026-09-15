// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::test_dump as bytes;

#[test]
fn definition_fixture_families_have_valid_nested_checksums() {
    for (version, archive) in [
        ("50", ArchiveVersion::V5),
        ("60", ArchiveVersion::V6),
        ("70", ArchiveVersion::V7),
        ("80", ArchiveVersion::V8),
    ] {
        let mut payloads = Vec::new();
        if archive.value() <= 60 {
            for minor in [6, 7] {
                payloads.push(bytes::v5_definition_payload(
                    archive,
                    minor,
                    [0x51; 16],
                    &[],
                    true,
                ));
            }
        }
        if archive.value() >= 60 {
            for (kind, linked, settings) in [
                (1, false, false),
                (2, true, false),
                (3, true, true),
                (0, false, false),
            ] {
                payloads.push(bytes::v6_definition_payload(
                    archive,
                    [0x51; 16],
                    &[],
                    kind,
                    linked,
                    settings,
                ));
            }
        }
        for (index, payload) in payloads.iter().enumerate() {
            let record = bytes::definition_record(archive, payload);
            let document = bytes::document_with_definitions(version, archive, &[record], &[]);
            for container_only in [false, true] {
                let result = RhinoCodec
                    .decode(
                        &mut Cursor::new(&document),
                        &DecodeOptions {
                            container_only,
                            ..DecodeOptions::default()
                        },
                    )
                    .expect("complete definition fixture");
                let integrity = result
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind())
                    .collect::<Vec<_>>();
                assert!(integrity.is_empty(), "archive={archive:?} fixture={index} container_only={container_only}: {integrity:?}");
                let _: cadmpeg_ir::document::CadIr =
                    serde_json::from_slice(&serde_json::to_vec(result.ir()).unwrap())
                        .expect("complete CADIR admission");
            }
        }
    }
}

fn definition_record(
    corrupt_units_crc: bool,
    malformed_tail: bool,
    meters_per_unit: f64,
) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let component = bytes::model_component_attributes(archive, [0x51; 16], 17, "definition");
    let mut units = bytes::unit_detail(archive, 2, meters_per_unit);
    if corrupt_units_crc {
        let last = units.len() - 1;
        units[last] ^= 1;
    }
    let versions = [1_i32.to_le_bytes(), 0_i32.to_le_bytes()].concat();
    let kind = 1_u32.to_le_bytes();
    let mut tail = bytes::utf16_bytes("description");
    tail.extend(bytes::utf16_bytes(""));
    tail.extend(bytes::utf16_bytes(""));
    for coordinate in [0.0_f64, 0.0, 0.0, 1.0, 2.0, 3.0] {
        tail.extend(coordinate.to_le_bytes());
    }
    // The member-array presence is a Boolean. 2 fails after unit admission.
    tail.push(if malformed_tail { 2 } else { 0 });
    tail.push(0); // no linked-type payload
    let mut direct = versions.clone();
    direct.extend(kind);
    direct.extend(&tail);
    let mut payload = versions;
    payload.extend(component);
    payload.extend(kind);
    payload.extend(units);
    payload.extend(tail);
    // Parent checksums cover only parent-level bytes, independently assembled here.
    payload.extend(crc32fast::hash(&direct).to_le_bytes());
    let definition = bytes::long_chunk(archive, 0x4000_8000, &payload);
    let mut class_data = definition;
    class_data.extend(0_u32.to_le_bytes()); // one nested child, no direct bytes
    let class = [
        bytes::crc_chunk(archive, 0x0002_fffb, &bytes::INSTANCE_DEFINITION_CLASS),
        bytes::long_chunk(archive, 0x0002_fffc, &class_data),
        bytes::short_chunk(archive, 0x8002_7fff, 0),
    ]
    .concat();
    let mut record = bytes::long_chunk(archive, 0x0002_7ffa, &class);
    record.extend(0_u32.to_le_bytes()); // only the nested class wrapper
    bytes::long_chunk(archive, 0x2000_8076, &record)
}

#[test]
fn definition_diagnostics_keep_codes_locations_and_prior_failures_in_both_decode_modes() {
    for corrupt_crc in [false, true] {
        for malformed_tail in [false, true] {
            let record = definition_record(corrupt_crc, malformed_tail, 0.001);
            let document = bytes::document_with_definitions(
                "80",
                ArchiveVersion::V8,
                std::slice::from_ref(&record),
                &[],
            );
            let scan = crate::container::scan_owned(document.clone()).expect("bounded definition");
            let source_offset = scan
                .tables
                .iter()
                .find(|table| table.typecode == 0x1000_0021)
                .unwrap()
                .records[0]
                .range
                .start as u64;
            assert_eq!(
                scan.definitions.definitions().len(),
                usize::from(!malformed_tail)
            );
            for container_only in [false, true] {
                let result = RhinoCodec
                    .decode(
                        &mut Cursor::new(&document),
                        &DecodeOptions {
                            container_only,
                            ..DecodeOptions::default()
                        },
                    )
                    .expect("definition diagnostics remain recoverable");
                let _: cadmpeg_ir::document::CadIr =
                    serde_json::from_slice(&serde_json::to_vec(result.ir()).unwrap())
                        .expect("complete CADIR admission");
                let integrity = result
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind())
                    .collect::<Vec<_>>();
                assert_eq!(integrity.len(), usize::from(corrupt_crc), "corrupt={corrupt_crc} malformed={malformed_tail} container_only={container_only}: {:?}", result.report().losses);
                if corrupt_crc {
                    assert_eq!(integrity[0].severity, Severity::Error);
                    let source = integrity[0]
                        .provenance
                        .as_ref()
                        .expect("located checksum diagnostic");
                    assert_eq!(source.offset, source_offset);
                    assert_eq!(source.tag.as_deref(), Some("INSTANCE_DEFINITION_TABLE"));
                }
                let structural = result
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| {
                        loss.code
                            == crate::loss::RhinoLossCode::ContainerInstanceDefinitionDegraded
                                .kind()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(structural.len(), usize::from(malformed_tail));
                if malformed_tail {
                    assert_eq!(
                        structural[0].provenance.as_ref().unwrap().offset,
                        source_offset
                    );
                    if !container_only {
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

#[test]
fn each_failed_definition_keeps_its_own_diagnostic_location() {
    let record = definition_record(true, true, 0.001);
    let document =
        bytes::document_with_definitions("80", ArchiveVersion::V8, &[record.clone(), record], &[]);
    let scan = crate::container::scan_owned(document.clone()).expect("two bounded definitions");
    let offsets = scan
        .tables
        .iter()
        .find(|table| table.typecode == 0x1000_0021)
        .unwrap()
        .records
        .iter()
        .map(|record| record.range.start as u64)
        .collect::<Vec<_>>();
    assert_eq!(offsets.len(), 2);
    for container_only in [false, true] {
        let result = RhinoCodec
            .decode(
                &mut Cursor::new(&document),
                &DecodeOptions {
                    container_only,
                    ..DecodeOptions::default()
                },
            )
            .expect("both failed definitions remain recoverable");
        for code in [
            crate::loss::RhinoLossCode::IntegrityFailure,
            crate::loss::RhinoLossCode::ContainerInstanceDefinitionDegraded,
        ] {
            let actual = result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == code.kind())
                .map(|loss| {
                    loss.provenance
                        .as_ref()
                        .expect("located definition diagnostic")
                        .offset
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual, offsets,
                "container_only={container_only} code={code:?}"
            );
        }
    }
}

#[test]
fn container_only_keeps_recorded_definition_field_losses() {
    let record = definition_record(false, false, 0.002);
    let document = bytes::document_with_definitions("80", ArchiveVersion::V8, &[record], &[]);
    for container_only in [false, true] {
        let result = RhinoCodec
            .decode(
                &mut Cursor::new(&document),
                &DecodeOptions {
                    container_only,
                    ..DecodeOptions::default()
                },
            )
            .expect("contradictory stored unit detail");
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::RhinoLossCode::RedundantFieldRepaired.kind()));
    }
}

#[test]
fn definition_field_losses_keep_each_record_location_before_later_failure() {
    for malformed_tail in [false, true] {
        let record = definition_record(false, malformed_tail, 0.002);
        let document = bytes::document_with_definitions(
            "80",
            ArchiveVersion::V8,
            &[record.clone(), record],
            &[],
        );
        let scan = crate::container::scan_owned(document.clone()).expect("bounded definitions");
        let offsets = scan
            .tables
            .iter()
            .find(|table| table.typecode == 0x1000_0021)
            .unwrap()
            .records
            .iter()
            .map(|record| record.range.start as u64)
            .collect::<Vec<_>>();
        assert_eq!(offsets.len(), 2);
        for container_only in [false, true] {
            let result = RhinoCodec
                .decode(
                    &mut Cursor::new(&document),
                    &DecodeOptions {
                        container_only,
                        ..DecodeOptions::default()
                    },
                )
                .expect("field losses survive a later parse failure or duplicate definition");
            let actual = result
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code == crate::loss::RhinoLossCode::RedundantFieldRepaired.kind()
                })
                .map(|loss| {
                    let provenance = loss.provenance.as_ref().expect("located field loss");
                    assert_eq!(provenance.tag.as_deref(), Some("INSTANCE_DEFINITION_TABLE"));
                    provenance.offset
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual, offsets,
                "malformed={malformed_tail} container_only={container_only}"
            );
            let _: cadmpeg_ir::document::CadIr =
                serde_json::from_slice(&serde_json::to_vec(result.ir()).unwrap())
                    .expect("complete CADIR admission");
        }
    }
}

#[test]
fn container_only_keeps_coded_container_checksum_failures() {
    let archive = ArchiveVersion::V8;
    let mut units = bytes::units_record(archive, 2);
    let last = units.len() - 1;
    units[last] ^= 1;
    let document = bytes::minimal_document(
        "80",
        &[
            bytes::table(archive, 0x1000_0014, &[]),
            bytes::table(archive, 0x1000_0015, &[units]),
            bytes::table(archive, 0x1000_0013, &[]),
        ],
    );
    for container_only in [false, true] {
        let result = RhinoCodec
            .decode(
                &mut Cursor::new(&document),
                &DecodeOptions {
                    container_only,
                    ..DecodeOptions::default()
                },
            )
            .expect("recoverable settings checksum mismatch");
        let failures = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == crate::loss::RhinoLossCode::IntegrityFailure.kind())
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1, "container_only={container_only}");
        assert_eq!(failures[0].severity, Severity::Error);
    }
}
