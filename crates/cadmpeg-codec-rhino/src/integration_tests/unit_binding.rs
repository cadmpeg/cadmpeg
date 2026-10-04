// SPDX-License-Identifier: Apache-2.0
use super::{assert_valid, decode};
use crate::chunks::ArchiveVersion;

fn unit_binding_document(unit: Option<i32>) -> (Vec<u8>, Vec<u8>) {
    let archive = ArchiveVersion::V8;
    let object = crate::test_support::test_dump::object_record_with_payload(
        archive,
        1,
        crate::test_support::test_dump::POINT_CLASS,
        &crate::test_support::test_dump::point_payload([1.0, 2.0, 3.0]),
    );
    let settings = unit
        .map(|value| vec![crate::test_support::test_dump::units_record(archive, value)])
        .unwrap_or_default();
    let bytes = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
            crate::test_support::test_dump::table(archive, 0x1000_0015, &settings),
            crate::test_support::test_dump::table(
                archive,
                0x1000_0013,
                std::slice::from_ref(&object),
            ),
        ],
    );
    (bytes, object)
}

#[test]
fn full_document_unit_binding_admits_only_physical_geometry() {
    for (unit, label, transferred) in [
        (Some(2), "millimeters", true),
        (Some(0), "native", false),
        (Some(255), "unavailable", false),
        (None, "unavailable", false),
    ] {
        let (bytes, object) = unit_binding_document(unit);
        let result = decode(bytes);
        assert_eq!(
            result.ir().model.points.len(),
            usize::from(transferred),
            "{label}"
        );
        let retained = result
            .source_fidelity()
            .retained_record("rhino:object:record#000000")
            .unwrap_or_else(|| panic!("{label} object source was not retained"));
        assert_eq!(retained.data(), Some(object.as_slice()), "{label}");
        if transferred {
            assert!(!result
                .report()
                .losses
                .iter()
                .any(|loss| { loss.message.contains("no physical millimetre binding") }));
        } else {
            assert!(result.report().losses.iter().any(|loss| {
                loss.code == crate::loss::RhinoLossCode::ObjectDecodeDiagnostic.kind()
                    && loss
                        .message
                        .contains(&format!("no physical millimetre binding ({label})"))
            }));
        }
        assert_valid(&result);
    }
}

#[test]
fn presentation_tables_retain_unit_dependent_records_without_binding() {
    let archive = ArchiveVersion::V8;
    for unit in [Some(0), Some(255), None] {
        let settings = unit
            .map(|value| vec![crate::test_support::test_dump::units_record(archive, value)])
            .unwrap_or_default();
        let table_specs = [
            (0x1000_0023, 0x2000_8078),
            (0x1000_0020, 0x2000_8075),
            (0x1000_0012, 0x2000_8060),
            (0x1000_0022, 0x2000_8077),
        ];
        let records = table_specs
            .iter()
            .map(|(_, typecode)| {
                crate::test_support::test_dump::crc_chunk(archive, *typecode, &[0xde, 0xad])
            })
            .collect::<Vec<_>>();
        let bytes = crate::test_support::test_dump::minimal_document(
            "80",
            &[
                crate::test_support::test_dump::table(archive, 0x1000_0014, &[]),
                crate::test_support::test_dump::table(archive, 0x1000_0015, &settings),
                crate::test_support::test_dump::table(
                    archive,
                    table_specs[0].0,
                    std::slice::from_ref(&records[0]),
                ),
                crate::test_support::test_dump::table(
                    archive,
                    table_specs[1].0,
                    std::slice::from_ref(&records[1]),
                ),
                crate::test_support::test_dump::table(
                    archive,
                    table_specs[2].0,
                    std::slice::from_ref(&records[2]),
                ),
                crate::test_support::test_dump::table(
                    archive,
                    table_specs[3].0,
                    std::slice::from_ref(&records[3]),
                ),
                crate::test_support::test_dump::table(archive, 0x1000_0013, &[]),
            ],
        );
        let result = decode(bytes);
        let namespace = result.ir().native.namespace("rhino").unwrap();
        for arena in ["lights", "linetypes", "hatch_patterns", "dimension_styles"] {
            assert!(
                namespace.arenas()[arena].is_empty(),
                "unit={unit:?} arena={arena}"
            );
        }
        for record in &records {
            assert!(
                result
                    .source_fidelity()
                    .retained_records()
                    .values()
                    .any(|retained| retained.data() == Some(record.as_slice())),
                "unit={unit:?} presentation source was not retained"
            );
        }
        assert!(result.report().losses.iter().any(|loss| {
            loss.code == crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
                && loss.message.contains("no physical millimetre binding")
        }));
        assert_valid(&result);
    }
}
