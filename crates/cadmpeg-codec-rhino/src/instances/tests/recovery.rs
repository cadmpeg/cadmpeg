// SPDX-License-Identifier: Apache-2.0
use crate::chunks::ArchiveVersion;
use crate::loss::RhinoLossCode;
use crate::test_support::test_dump as bytes;
use crate::wire::Uuid;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use std::io::Cursor;

#[derive(Clone, Copy, Debug)]
enum Metadata {
    Valid,
    InvalidCustomScale,
    FutureUnitVersion,
    ShortUnitPayload,
    InvalidComponentStatus,
    NonFiniteBounds,
    InvalidUtf16,
    MissingStringTerminator,
}

fn definition(id: [u8; 16], member: [u8; 16], metadata: Metadata) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let versions = [1_i32.to_le_bytes(), 0_i32.to_le_bytes()].concat();
    let mut component = versions.clone();
    component.push(if matches!(metadata, Metadata::InvalidComponentStatus) {
        3
    } else {
        0
    });
    component.push(1);
    component.extend(id);
    component.extend([0, 0, 0]); // absent component type, index, and name
    let component = bytes::crc_chunk(archive, 0x4000_8002, &component);
    let units = match metadata {
        Metadata::InvalidCustomScale => bytes::unit_detail(archive, 11, 0.0),
        Metadata::FutureUnitVersion => bytes::crc_chunk(
            archive,
            0x4000_8000,
            &[2_i32.to_le_bytes(), 0_i32.to_le_bytes()].concat(),
        ),
        Metadata::ShortUnitPayload => bytes::crc_chunk(archive, 0x4000_8000, &[]),
        _ => bytes::unit_detail(archive, 2, 0.001),
    };
    let mut tail = Vec::new();
    for index in 0..3 {
        if index == 0
            && matches!(
                metadata,
                Metadata::InvalidUtf16 | Metadata::MissingStringTerminator
            )
        {
            tail.extend(2_u32.to_le_bytes());
            tail.extend(
                if matches!(metadata, Metadata::InvalidUtf16) {
                    0xd800_u16
                } else {
                    0x41_u16
                }
                .to_le_bytes(),
            );
            tail.extend(
                if matches!(metadata, Metadata::MissingStringTerminator) {
                    0x42_u16
                } else {
                    0_u16
                }
                .to_le_bytes(),
            );
        } else {
            tail.extend(bytes::utf16_bytes(""));
        }
    }
    let minimum_x = if matches!(metadata, Metadata::NonFiniteBounds) {
        f64::NAN
    } else {
        0.0
    };
    for coordinate in [minimum_x, 0.0, 0.0, 1.0, 1.0, 1.0] {
        tail.extend(coordinate.to_le_bytes());
    }
    tail.push(1);
    tail.extend(1_i32.to_le_bytes());
    tail.extend(member);
    tail.push(0); // no linked payload
    let kind = 1_u32.to_le_bytes();
    let direct = [versions.as_slice(), &kind, &tail].concat();
    let mut outer = [versions, component, kind.to_vec(), units, tail].concat();
    outer.extend(crc32fast::hash(&direct).to_le_bytes());
    let mut class_data = bytes::long_chunk(archive, 0x4000_8000, &outer);
    class_data.extend(0_u32.to_le_bytes());
    let class = [
        bytes::crc_chunk(archive, 0x0002_fffb, &bytes::INSTANCE_DEFINITION_CLASS),
        bytes::long_chunk(archive, 0x0002_fffc, &class_data),
        bytes::short_chunk(archive, 0x8002_7fff, 0),
    ]
    .concat();
    let mut record = bytes::long_chunk(archive, 0x0002_7ffa, &class);
    record.extend(0_u32.to_le_bytes());
    bytes::long_chunk(archive, 0x2000_8076, &record)
}

fn point(id: [u8; 16], x: f64) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    let mut attributes = bytes::tagged_attributes(&[], 0);
    attributes[1..17].copy_from_slice(&id);
    let body = [
        bytes::short_chunk(archive, 0x8200_0071, 1),
        bytes::class_wrapper(
            archive,
            bytes::POINT_CLASS,
            &bytes::point_payload([x, 0.0, 0.0]),
        ),
        bytes::crc_chunk(archive, 0x0200_8072, &attributes),
        bytes::short_chunk(archive, 0x8200_007f, 0),
    ]
    .concat();
    bytes::nested_crc_chunk(archive, 0x2000_8070, &body)
}

fn document(definitions: &[Vec<u8>], objects: &[Vec<u8>]) -> Vec<u8> {
    let archive = ArchiveVersion::V8;
    bytes::minimal_document(
        "80",
        &[
            bytes::table(archive, 0x1000_0014, &[]),
            bytes::table(archive, 0x1000_0015, &[bytes::units_record(archive, 2)]),
            bytes::table(archive, 0x1000_0021, definitions),
            bytes::table(archive, 0x1000_0013, objects),
        ],
    )
}

#[test]
fn duplicate_definitions_retain_every_record_and_locate_every_ambiguity() {
    for count in [2, 3] {
        let duplicate = [0x51; 16];
        let records: Vec<_> = (0..count)
            .map(|i| definition(duplicate, [0x60 + i; 16], Metadata::Valid))
            .collect();
        let source = document(&records, &[]);
        let scan = crate::container::scan_owned(source.clone()).unwrap();
        let offsets: Vec<_> = scan
            .tables
            .iter()
            .find(|table| table.typecode == 0x1000_0021)
            .unwrap()
            .records
            .iter()
            .map(|record| record.range.start as u64)
            .collect();
        assert!(scan.definitions.definitions.is_empty());
        assert!(scan
            .definitions
            .ambiguous_ids
            .contains(&Uuid::from_wire(duplicate)));
        for container_only in [false, true] {
            let decoded = crate::RhinoCodec
                .decode(
                    &mut Cursor::new(&source),
                    &DecodeOptions {
                        container_only,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            let losses: Vec<_> = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code == RhinoLossCode::ContainerInstanceDefinitionDegraded.kind()
                })
                .collect();
            assert_eq!(losses.len(), usize::from(count));
            let mut actual: Vec<_> = losses
                .iter()
                .map(|loss| {
                    assert!(loss
                        .message
                        .contains(&Uuid::from_wire(duplicate).to_string()));
                    loss.provenance.as_ref().unwrap().offset
                })
                .collect();
            actual.sort_unstable();
            assert_eq!(actual, offsets);
            assert!(!decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == RhinoLossCode::IntegrityFailure.kind()));
            let _: CadIr = CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
            if !container_only {
                for (index, record) in records.iter().enumerate() {
                    let retained: Vec<_> = decoded
                        .source_fidelity()
                        .retained_records()
                        .values()
                        .filter(|retained| retained.offset() == offsets[index])
                        .collect();
                    assert_eq!(retained.len(), 1);
                    assert_eq!(retained[0].data(), Some(record.as_slice()));
                }
            }
        }
    }
}

#[test]
fn bounded_definition_members_do_not_become_ordinary_geometry_after_metadata_failure() {
    for metadata in [
        Metadata::Valid,
        Metadata::InvalidCustomScale,
        Metadata::FutureUnitVersion,
        Metadata::ShortUnitPayload,
        Metadata::InvalidComponentStatus,
        Metadata::NonFiniteBounds,
        Metadata::InvalidUtf16,
        Metadata::MissingStringTerminator,
    ] {
        let member = [0x61; 16];
        let ordinary = [0x62; 16];
        let record = definition([0x51; 16], member, metadata);
        let source = document(
            std::slice::from_ref(&record),
            &[point(member, 3.0), point(ordinary, 9.0)],
        );
        let scan = crate::container::scan_owned(source.clone()).unwrap();
        let offset = scan
            .tables
            .iter()
            .find(|table| table.typecode == 0x1000_0021)
            .unwrap()
            .records[0]
            .range
            .start as u64;
        let decoded = crate::RhinoCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        assert!(
            !decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == RhinoLossCode::IntegrityFailure.kind()),
            "{metadata:?}: {:?}",
            decoded.report().losses
        );
        let degraded: Vec<_> = decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == RhinoLossCode::ContainerInstanceDefinitionDegraded.kind())
            .collect();
        assert_eq!(
            degraded.len(),
            usize::from(!matches!(metadata, Metadata::Valid)),
            "{metadata:?}: {degraded:?}"
        );
        for loss in degraded {
            assert_eq!(loss.provenance.as_ref().unwrap().offset, offset);
        }
        let reread = CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
        for ir in [decoded.ir(), &reread] {
            assert_eq!(ir.model.points.len(), 1, "{metadata:?}");
            assert_eq!(ir.model.points[0].position.x, 9.0);
        }
        assert!(
            scan.definitions
                .member_object_ids
                .contains(&Uuid::from_wire(member)),
            "{metadata:?}"
        );
        assert!(!scan
            .definitions
            .member_object_ids
            .contains(&Uuid::from_wire(ordinary)));
        if !matches!(metadata, Metadata::Valid) {
            assert!(decoded
                .source_fidelity()
                .retained_records()
                .values()
                .any(|source| source.data() == Some(record.as_slice())));
        }
    }
}

#[test]
fn nil_definition_identity_is_not_admitted_and_keeps_source_membership() {
    for (archive, version) in [(ArchiveVersion::V5, "50"), (ArchiveVersion::V8, "80")] {
        let member = [0x61; 16];
        let ordinary = [0x62; 16];
        let record = if archive == ArchiveVersion::V5 {
            bytes::definition_record(
                archive,
                &bytes::v5_definition_payload(archive, 6, [0; 16], &[member], false),
            )
        } else {
            definition([0; 16], member, Metadata::Valid)
        };
        let source = bytes::minimal_document(
            version,
            &[
                bytes::table(archive, 0x1000_0014, &[]),
                bytes::table(archive, 0x1000_0015, &[bytes::units_record(archive, 2)]),
                bytes::table(archive, 0x1000_0021, std::slice::from_ref(&record)),
                bytes::table(
                    archive,
                    0x1000_0013,
                    &[point(member, 3.0), point(ordinary, 9.0)],
                ),
            ],
        );
        let scan = crate::container::scan_owned(source.clone()).unwrap();
        let offset = scan
            .tables
            .iter()
            .find(|table| table.typecode == 0x1000_0021)
            .unwrap()
            .records[0]
            .range
            .start as u64;
        for container_only in [false, true] {
            let decoded = crate::RhinoCodec
                .decode(
                    &mut Cursor::new(&source),
                    &DecodeOptions {
                        container_only,
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            assert!(scan.definitions.definitions().is_empty());
            assert!(scan.definitions.contains_member(Uuid::from_wire(member)));
            assert!(!scan.definitions.contains_member(Uuid::from_wire(ordinary)));
            let losses: Vec<_> = decoded
                .report()
                .losses
                .iter()
                .filter(|loss| {
                    loss.code == RhinoLossCode::ContainerInstanceDefinitionDegraded.kind()
                })
                .collect();
            assert_eq!(losses.len(), 1);
            assert!(losses[0].message.contains("definition UUID is nil"));
            assert_eq!(losses[0].provenance.as_ref().unwrap().offset, offset);
            assert!(!decoded
                .report()
                .losses
                .iter()
                .any(|loss| { loss.code == RhinoLossCode::IntegrityFailure.kind() }));
            let reread = CadIr::from_json(&serde_json::to_string(decoded.ir()).unwrap()).unwrap();
            for ir in [decoded.ir(), &reread] {
                assert_eq!(ir.model.points.len(), usize::from(!container_only));
                if !container_only {
                    assert_eq!(ir.model.points[0].position.x, 9.0);
                }
            }
            if !container_only {
                assert!(decoded
                    .source_fidelity()
                    .retained_records()
                    .values()
                    .any(|retained| retained.data() == Some(record.as_slice())));
            }
        }
    }
}
