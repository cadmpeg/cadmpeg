// SPDX-License-Identifier: Apache-2.0
#![allow(dead_code, clippy::disallowed_methods)]

use super::{
    anonymous, file_reference as parse_file_reference, parse_reference, scale_translation,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::transform::Transform;

use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::test_support::test_dump::*;
use crate::wire::Uuid;

const OBSOLETE_IDEF_LAYER_SETTINGS: Uuid = Uuid::from_canonical([
    0x11, 0xee, 0x2c, 0x1f, 0xf9, 0x0d, 0x4c, 0x6a, 0xa7, 0xcd, 0xec, 0x85, 0x32, 0xe1, 0xe3, 0x2d,
]);

#[test]
fn parent_child_composition_uses_column_point_order() {
    let parent = Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("affine transform");
    let child = Transform::from_rows([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .expect("affine transform");
    assert_eq!(
        parent
            .compose(child)
            .apply_point(Point3::new(1.0, 0.0, 0.0)),
        Point3::new(12.0, 0.0, 0.0)
    );
}

#[test]
fn translation_scales_once_without_scaling_linear_coefficients() {
    let source = Transform::from_rows([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .expect("affine transform");
    let scaled = scale_translation(source, 25.4).expect("finite translation");
    assert_eq!(scaled.rows()[0][0], 2.0);
    assert_eq!(scaled.rows()[1][3], 76.199_999_999_999_99);
}

#[test]
fn translation_scaling_rejects_overflow() {
    let source = Transform::affine([
        [1.0, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("affine transform");
    assert!(scale_translation(source, 2.0).is_none());
}

#[test]
fn anonymous_instance_crc_mismatch_warns_and_consumes_boundary() {
    let body = [1_i32.to_le_bytes(), 0_i32.to_le_bytes()].concat();
    let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(
        &i64::try_from(body.len() + 4)
            .expect("required invariant")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&crc32fast::hash(&body).to_le_bytes());
    let crc = bytes.len() - 1;
    bytes[crc] ^= 1;
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("required invariant");
    let mut warnings = Vec::new();
    let (_, payload) = anonymous(
        &bytes,
        &mut reader,
        ArchiveVersion::V5,
        "instance test",
        &mut warnings,
    )
    .expect("recoverable anonymous chunk");
    assert_eq!(reader.remaining(), 0);
    assert_eq!(payload.remaining(), 0);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("instance test CRC mismatch"));
}

#[test]
fn normals_use_inverse_transpose_and_normalization() {
    let transform = Transform::from_rows([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.5, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .expect("affine transform");
    assert_eq!(
        transform.apply_normal(Vector3::new(1.0, 0.0, 1.0)),
        Some(Vector3::new(
            0.242_535_625_036_332_97,
            0.0,
            0.970_142_500_145_331_9
        ))
    );
}

fn reference_bytes(transform: Transform) -> Vec<u8> {
    reference_matrix_bytes(transform.rows())
}

fn reference_matrix_bytes(rows: [[f64; 4]; 4]) -> Vec<u8> {
    let mut bytes = vec![0x10];
    bytes.extend_from_slice(&[
        0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ]);
    for value in rows.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn set_anonymous_minor(chunk: &mut [u8], minor: i32) {
    chunk[16..20].copy_from_slice(&minor.to_le_bytes());
}

fn append_crc_suffix(chunk: &mut Vec<u8>, suffix: &[u8]) {
    let crc_offset = chunk.len() - 4;
    chunk.splice(crc_offset..crc_offset, suffix.iter().copied());
    let length = i64::from_le_bytes(chunk[4..12].try_into().expect("chunk header"));
    chunk[4..12].copy_from_slice(&(length + suffix.len() as i64).to_le_bytes());
    let crc = crc32fast::hash(&chunk[12..chunk.len() - 4]);
    let crc_offset = chunk.len() - 4;
    chunk[crc_offset..].copy_from_slice(&crc.to_le_bytes());
}

#[test]
fn instance_reference_requires_finite_invertible_affine_payload_and_skips_future_suffix() {
    let valid = reference_bytes(Transform::identity());
    let parsed = parse_reference(&valid, 0..valid.len()).expect("required invariant");
    assert_eq!(
        parsed.definition_id.to_string(),
        "00112233-4455-6677-8899-aabbccddeeff"
    );
    assert_eq!(parsed.transform, Transform::identity());

    let mut singular = Transform::identity().rows();
    singular[2][2] = 0.0;
    let singular = reference_matrix_bytes(singular);
    assert!(parse_reference(&singular, 0..singular.len()).is_err());

    let mut projective = Transform::identity().rows();
    projective[3][0] = 1.0;
    let projective = reference_matrix_bytes(projective);
    assert!(parse_reference(&projective, 0..projective.len()).is_err());

    let mut trailing = valid;
    trailing[0] = 0x1f;
    trailing.push(0);
    let parsed = parse_reference(&trailing, 0..trailing.len()).expect("future suffix is bounded");
    assert_eq!(parsed.transform, Transform::identity());
}

#[test]
fn instance_reference_rejects_nil_definition_and_nonfinite_transform() {
    let mut nil = reference_bytes(Transform::identity());
    nil[1..17].fill(0);
    assert!(parse_reference(&nil, 0..nil.len()).is_err());

    let mut nonfinite = Transform::identity().rows();
    nonfinite[1][2] = f64::NAN;
    let nonfinite = reference_matrix_bytes(nonfinite);
    assert!(parse_reference(&nonfinite, 0..nonfinite.len()).is_err());
}

#[test]
fn instance_definition_readers_follow_source_minor_boundaries() {
    let definition_id = [0x10; 16];
    let member_id = [0x20; 16];

    let mut v5_payload =
        v5_definition_payload(ArchiveVersion::V5, 7, definition_id, &[member_id], true);
    v5_payload.extend([0xde, 0xad, 0xbe, 0xef]);
    let v5_record = definition_record(ArchiveVersion::V5, &v5_payload);
    let scan = crate::container::scan_owned(document_with_definitions(
        "50",
        ArchiveVersion::V5,
        &[v5_record],
        &[],
    ))
    .expect("required invariant");
    assert_eq!(scan.definitions.definitions.len(), 1);
    assert!(scan.definitions.definitions[0].file_reference.is_some());

    let mut future_payload =
        v5_definition_payload(ArchiveVersion::V5, 6, definition_id, &[], false);
    future_payload[0] = 0x20;
    let future_record = definition_record(ArchiveVersion::V5, &future_payload);
    let scan = crate::container::scan_owned(document_with_definitions(
        "50",
        ArchiveVersion::V5,
        std::slice::from_ref(&future_record),
        &[],
    ))
    .expect("future instance-definition record");
    assert!(scan.definitions.definitions.is_empty());
    let retained = scan
        .opaque_records
        .iter()
        .find(|record| record.table_typecode & !crate::chunks::TCODE_CRC == 0x1000_0021)
        .expect("future instance-definition record is retained");
    assert_eq!(retained.record.typecode, 0x2000_8076);
    assert_eq!(
        &scan.data[retained.record.range.clone()],
        future_record.as_slice()
    );

    let mut v6_payload = v6_definition_payload(
        ArchiveVersion::V7,
        [0x30; 16],
        &[member_id],
        1,
        false,
        false,
    );
    set_anonymous_minor(&mut v6_payload, 9);
    append_crc_suffix(&mut v6_payload, &[0xca, 0xfe]);
    let v6_record = definition_record(ArchiveVersion::V7, &v6_payload);
    let scan = crate::container::scan_owned(document_with_definitions(
        "70",
        ArchiveVersion::V7,
        &[v6_record],
        &[],
    ))
    .expect("required invariant");
    assert_eq!(scan.definitions.definitions.len(), 1);
    assert_eq!(
        scan.definitions.definitions[0].members,
        vec![Uuid::from_wire(member_id)]
    );

    let mut reference = file_reference(ArchiveVersion::V6, "/full/source.3dm", "source.3dm");
    set_anonymous_minor(&mut reference, 9);
    append_crc_suffix(&mut reference, &[0xa5, 0x5a]);
    let mut reader = BoundedReader::new(&reference, 0, reference.len()).expect("chunk bounds");
    let parsed = parse_file_reference(&reference, &mut reader, ArchiveVersion::V6, &mut Vec::new())
        .expect("future file-reference suffix is bounded");
    assert_eq!(parsed.full_path, "/full/source.3dm");
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn obsolete_idef_layer_settings_are_consumed_without_definition_fields() {
    let archive = ArchiveVersion::V5;
    let application_uuid = super::OPENNURBS5_APPLICATION.to_wire();
    for major in [1, 2] {
        let definition_id = [0x51; 16];
        let payload = v5_definition_payload_with_paths(
            archive,
            6,
            definition_id,
            &[],
            true,
            "/full/source.3dm",
            false,
        );
        let userdata = class_userdata_with_anonymous_payload(
            archive,
            OBSOLETE_IDEF_LAYER_SETTINGS.to_wire(),
            application_uuid,
            major,
            &[0xde, 0xad],
        );
        let record = definition_record_with_userdata(archive, &payload, &userdata);
        let scan = crate::container::scan_owned(document_with_definitions(
            "50",
            archive,
            std::slice::from_ref(&record),
            &[],
        ))
        .expect("obsolete instance-definition userdata witness");
        assert_eq!(scan.definitions.definitions.len(), 1);
        let definition = &scan.definitions.definitions[0];
        assert_eq!(definition.kind, super::DefinitionKind::Linked);
        assert_eq!(definition.legacy_linked_path, "/full/source.3dm");
        assert!(scan.definitions.diagnostics.is_empty());
        assert!(scan.opaque_records.is_empty());
    }

    let definition_id = [0x52; 16];
    let payload = v5_definition_payload_with_paths(
        archive,
        6,
        definition_id,
        &[],
        true,
        "/full/source.3dm",
        false,
    );
    let malformed = class_userdata_v2_with_direct_payload(
        archive,
        OBSOLETE_IDEF_LAYER_SETTINGS.to_wire(),
        application_uuid,
        50,
        202_608_010,
        &[0xde, 0xad],
    );
    let record = definition_record_with_userdata(archive, &payload, &malformed);
    let scan = crate::container::scan_owned(document_with_definitions(
        "50",
        archive,
        std::slice::from_ref(&record),
        &[],
    ))
    .expect("malformed obsolete instance-definition userdata witness");
    assert_eq!(scan.definitions.definitions.len(), 1);
    assert_eq!(
        scan.definitions.definitions[0].kind,
        super::DefinitionKind::Linked
    );
    assert!(scan.definitions.diagnostics.is_empty());
    assert!(scan.opaque_records.is_empty());
}

#[test]
fn obsolete_alternative_path_userdata_applies_v5_slot_precedence() {
    let archive = ArchiveVersion::V5;
    let definition_id = [0x41; 16];
    let class_uuid = super::IDEF_ALTERNATIVE_PATH_USERDATA.to_wire();
    let application_uuid = super::OPENNURBS5_APPLICATION.to_wire();

    let relative_carrier = class_userdata(
        archive,
        class_uuid,
        application_uuid,
        "  relative/source.3dm  ",
        true,
    );
    let relative_payload = v5_definition_payload_with_paths(
        archive,
        6,
        definition_id,
        &[],
        true,
        "/full/source.3dm",
        false,
    );
    let record = definition_record_with_userdata(archive, &relative_payload, &relative_carrier);
    let mut scan =
        crate::container::scan_owned(document_with_definitions("50", archive, &[record], &[]))
            .expect("V5 alternate-path witness");
    let parsed = &scan.definitions.definitions[0];
    assert_eq!(parsed.kind, crate::instances::DefinitionKind::Linked);
    assert_eq!(parsed.legacy_linked_path, "/full/source.3dm");
    assert_eq!(parsed.legacy_relative_linked_path, "relative/source.3dm");
    assert!(parsed.legacy_relative_path);
    assert!(scan.definitions.diagnostics.is_empty());
    set_test_units(&mut scan, 1.0);
    let result = crate::decode::decode_for_test(&scan);
    let external = &result
        .ir()
        .native
        .namespace("rhino")
        .expect("Rhino native namespace")
        .arenas["external_references"][0];
    assert_eq!(external.fields()["full_path"], "/full/source.3dm");
    assert_eq!(external.fields()["relative_path"], "relative/source.3dm");
    assert_eq!(external.fields()["relative_path_preferred"], true);
    assert!(result.source_fidelity().retained_records.is_empty());

    let full_carrier = class_userdata(
        archive,
        class_uuid,
        application_uuid,
        "/replacement/source.3dm",
        false,
    );
    let occupied_full = v5_definition_payload_with_paths(
        archive,
        6,
        [0x42; 16],
        &[],
        true,
        "/full/original.3dm",
        false,
    );
    let occupied_full_record =
        definition_record_with_userdata(archive, &occupied_full, &full_carrier);
    let scan = crate::container::scan_owned(document_with_definitions(
        "50",
        archive,
        &[occupied_full_record],
        &[],
    ))
    .expect("occupied full-path witness");
    let parsed = &scan.definitions.definitions[0];
    assert_eq!(parsed.legacy_linked_path, "/full/original.3dm");
    assert!(parsed.legacy_relative_linked_path.is_empty());

    let relative_base = v5_definition_payload_with_paths(
        archive,
        6,
        [0x43; 16],
        &[],
        true,
        "relative/base.3dm",
        true,
    );
    let relative_base_record =
        definition_record_with_userdata(archive, &relative_base, &full_carrier);
    let scan = crate::container::scan_owned(document_with_definitions(
        "50",
        archive,
        &[relative_base_record],
        &[],
    ))
    .expect("relative-base witness");
    let parsed = &scan.definitions.definitions[0];
    assert_eq!(parsed.legacy_linked_path, "/replacement/source.3dm");
    assert_eq!(parsed.legacy_relative_linked_path, "relative/base.3dm");
    assert!(parsed.legacy_relative_path);

    let static_payload = v5_definition_payload_with_paths(
        archive,
        6,
        [0x44; 16],
        &[],
        false,
        "ignored-on-static-definition.3dm",
        false,
    );
    let static_record = definition_record_with_userdata(archive, &static_payload, &full_carrier);
    let scan = crate::container::scan_owned(document_with_definitions(
        "50",
        archive,
        &[static_record],
        &[],
    ))
    .expect("static-path witness");
    let parsed = &scan.definitions.definitions[0];
    assert_eq!(parsed.kind, crate::instances::DefinitionKind::Static);
    assert!(parsed.legacy_linked_path.is_empty());
    assert!(parsed.legacy_relative_linked_path.is_empty());

    let malformed_body = utf16_bytes("/ignored/malformed.3dm");
    let malformed_carrier =
        class_userdata_with_payload(archive, class_uuid, application_uuid, &malformed_body);
    let malformed_payload = v5_definition_payload_with_paths(
        archive,
        6,
        [0x45; 16],
        &[],
        true,
        "/retained/source.3dm",
        false,
    );
    let malformed_record =
        definition_record_with_userdata(archive, &malformed_payload, &malformed_carrier);
    let malformed_document = document_with_definitions("50", archive, &[malformed_record], &[]);
    let malformed_source_bytes = malformed_document.clone();
    let mut scan = crate::container::scan_owned(malformed_document)
        .expect("malformed optional carrier remains framed");
    let parsed = &scan.definitions.definitions[0];
    assert_eq!(parsed.legacy_linked_path, "/retained/source.3dm");
    assert!(parsed.legacy_relative_linked_path.is_empty());
    assert!(scan
        .definitions
        .diagnostics
        .iter()
        .any(
            |diagnostic| diagnostic.message.contains("alternate-path userdata")
                && diagnostic.message.contains("was dropped")
        ));
    let malformed_range = scan
        .opaque_records
        .iter()
        .find(|source| source.record.typecode == 0x2000_8076)
        .map(|source| source.record.range.clone())
        .expect("malformed definition record is retained");
    assert_eq!(
        &scan.data[malformed_range.clone()],
        &malformed_source_bytes[malformed_range.clone()]
    );
    set_test_units(&mut scan, 1.0);
    let malformed_result = crate::decode::decode_for_test(&scan);
    let malformed_retained = malformed_result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|source| source.offset() == malformed_range.start as u64)
        .expect("malformed definition fidelity");
    assert_eq!(
        malformed_retained.data(),
        Some(&malformed_source_bytes[malformed_range])
    );

    let future_body = [
        utf16_bytes(" relative/future.3dm ").as_slice(),
        [1].as_slice(),
    ]
    .concat();
    let future_carrier = class_userdata_with_anonymous_payload(
        archive,
        class_uuid,
        application_uuid,
        2,
        &future_body,
    );
    let future_payload = v5_definition_payload_with_paths(
        archive,
        6,
        [0x46; 16],
        &[],
        true,
        "/future/full.3dm",
        false,
    );
    let future_record = definition_record_with_userdata(archive, &future_payload, &future_carrier);
    let future_document =
        document_with_definitions("50", archive, std::slice::from_ref(&future_record), &[]);
    let mut future_scan = crate::container::scan_owned(future_document)
        .expect("future optional carrier remains framed");
    let future_definition = &future_scan.definitions.definitions[0];
    assert_eq!(future_definition.legacy_linked_path, "/future/full.3dm");
    assert!(future_definition.legacy_relative_linked_path.is_empty());
    assert!(future_scan
        .definitions
        .diagnostics
        .iter()
        .any(|diagnostic| {
            diagnostic
                .message
                .contains("unsupported instance-definition alternate-path version")
        }));
    let future_range = future_scan
        .opaque_records
        .iter()
        .find(|source| source.record.typecode == 0x2000_8076)
        .map(|source| source.record.range.clone())
        .expect("future definition record is retained");
    assert_eq!(
        &future_scan.data[future_range.clone()],
        future_record.as_slice()
    );
    set_test_units(&mut future_scan, 1.0);
    let future_result = crate::decode::decode_for_test(&future_scan);
    let future_external = &future_result
        .ir()
        .native
        .namespace("rhino")
        .expect("Rhino native namespace")
        .arenas["external_references"][0];
    assert_eq!(future_external.fields()["relative_path"], "");
    let future_retained = future_result
        .source_fidelity()
        .retained_records
        .iter()
        .find(|source| source.offset() == future_range.start as u64)
        .expect("future definition fidelity");
    assert_eq!(future_retained.data(), Some(future_record.as_slice()));
}

#[test]
pub(crate) fn parses_source_shaped_v5_minor_6_and_7_definition_records() {
    let definition_id = [0x10; 16];
    let member_id = [0x20; 16];
    let v5 = definition_record(
        ArchiveVersion::V5,
        &v5_definition_payload(ArchiveVersion::V5, 6, definition_id, &[member_id], true),
    );
    let scan = crate::container::scan_owned(document_with_definitions(
        "50",
        ArchiveVersion::V5,
        &[v5],
        &[],
    ))
    .expect("required invariant");
    let parsed = &scan.definitions.definitions[0];
    assert_eq!(parsed.kind, crate::instances::DefinitionKind::Linked);
    assert_eq!(parsed.members, vec![Uuid::from_wire(member_id)]);
    assert_eq!(parsed.units.unit, 2);
    assert_eq!(parsed.units.meters_per_unit, 0.001);
    assert_eq!(parsed.linked_appearance, 2);
    assert_eq!(
        parsed
            .legacy_checksum_range
            .as_ref()
            .expect("required invariant")
            .len(),
        48
    );
    assert!(parsed.file_reference_range.is_none());

    let v6 = definition_record(
        ArchiveVersion::V6,
        &v5_definition_payload(ArchiveVersion::V6, 7, definition_id, &[member_id], true),
    );
    let scan = crate::container::scan_owned(document_with_definitions(
        "60",
        ArchiveVersion::V6,
        &[v6],
        &[],
    ))
    .expect("required invariant");
    let parsed = &scan.definitions.definitions[0];
    assert!(parsed.file_reference_range.is_some());
}

#[test]
pub(crate) fn parses_source_shaped_v6_v7_v8_static_and_linked_definitions() {
    for (version, archive) in [
        ("60", ArchiveVersion::V6),
        ("70", ArchiveVersion::V7),
        ("80", ArchiveVersion::V8),
    ] {
        let definition_id = [archive.value() as u8; 16];
        let member_id = [archive.value() as u8 + 1; 16];
        let static_record = definition_record(
            archive,
            &v6_definition_payload(archive, definition_id, &[member_id], 1, false, false),
        );
        let linked_record = definition_record(
            archive,
            &v6_definition_payload(archive, [0x70; 16], &[], 3, true, true),
        );
        let embedded_record = definition_record(
            archive,
            &v6_definition_payload(archive, [0x71; 16], &[], 2, true, false),
        );
        let unset_record = definition_record(
            archive,
            &v6_definition_payload(archive, [0x72; 16], &[], 0, false, false),
        );
        let scan = crate::container::scan_owned(document_with_definitions(
            version,
            archive,
            &[static_record, linked_record, embedded_record, unset_record],
            &[],
        ))
        .expect("required invariant");
        assert_eq!(scan.definitions.definitions.len(), 4);
        let static_definition = &scan.definitions.definitions[0];
        assert_eq!(
            static_definition.kind,
            crate::instances::DefinitionKind::Static
        );
        assert_eq!(static_definition.index, Some(17));
        assert_eq!(static_definition.name, "modern definition");
        assert_eq!(static_definition.members, vec![Uuid::from_wire(member_id)]);
        assert_eq!(static_definition.units.unit, 8);
        assert_eq!(static_definition.units.meters_per_unit, 0.0254);
        let linked = &scan.definitions.definitions[1];
        assert_eq!(linked.kind, crate::instances::DefinitionKind::Linked);
        assert!(linked.members.is_empty());
        assert_eq!(linked.linked_depth, 2);
        assert_eq!(linked.linked_appearance, 2);
        assert!(linked.reference_settings_range.is_some());
        assert!(linked.file_reference_range.is_some());
        assert_eq!(
            scan.definitions.definitions[2].kind,
            crate::instances::DefinitionKind::LinkedAndEmbedded
        );
        assert_eq!(
            scan.definitions.definitions[3].kind,
            crate::instances::DefinitionKind::Unset
        );
    }
}

#[test]
fn definition_scan_recovers_after_malformed_record_and_preserves_membership_union() {
    let archive = ArchiveVersion::V7;
    let duplicate_id = [0x31; 16];
    let first_member = [0x41; 16];
    let second_member = [0x42; 16];
    let malformed_member = [0x43; 16];
    let ordinary_member = [0x44; 16];
    let first = definition_record(
        archive,
        &v6_definition_payload(archive, duplicate_id, &[first_member], 1, false, false),
    );
    let second = definition_record(
        archive,
        &v6_definition_payload(archive, duplicate_id, &[second_member], 1, false, false),
    );
    let mut malformed_payload =
        v6_definition_payload(archive, [0x32; 16], &[malformed_member], 2, true, false);
    let invalid_settings_flag = malformed_payload.len() - 9;
    malformed_payload[invalid_settings_flag] = 2;
    let malformed = definition_record(archive, &malformed_payload);
    let later = definition_record(
        archive,
        &v6_definition_payload(archive, [0x33; 16], &[], 1, false, false),
    );
    let objects = [
        first_member,
        second_member,
        malformed_member,
        ordinary_member,
    ]
    .map(|_| object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 0.0, 0.0])));
    let mut scan = crate::container::scan_owned(document_with_definitions(
        "70",
        archive,
        &[first, second, malformed, later],
        &objects,
    ))
    .expect("required invariant");
    assert!(scan
        .definitions
        .ambiguous_ids
        .contains(&Uuid::from_wire(duplicate_id)));
    for member in [first_member, second_member, malformed_member] {
        assert!(scan
            .definitions
            .member_object_ids
            .contains(&Uuid::from_wire(member)));
    }
    assert_eq!(scan.definitions.definitions.len(), 1);
    assert!(scan.definitions.diagnostics.len() >= 2);
    assert!(scan.definitions.diagnostics.iter().all(|diagnostic| {
        diagnostic.source_range.start < diagnostic.source_range.end
            && !diagnostic.message.contains("unsupported class")
    }));
    let container_only =
        crate::decode::seal_for_test(crate::container::container_only_result(&scan), true);
    assert!(container_only.report().losses.iter().any(|loss| {
        loss.severity == Severity::Warning
            && loss
                .provenance
                .as_ref()
                .and_then(|value| value.tag.as_deref())
                == Some("INSTANCE_DEFINITION_TABLE")
    }));
    for (source_order, id) in [
        first_member,
        second_member,
        malformed_member,
        ordinary_member,
    ]
    .into_iter()
    .enumerate()
    {
        set_identity(
            &mut scan,
            source_order,
            id,
            &format!("definition-member-{source_order}"),
            None,
            true,
        );
    }
    set_test_units(&mut scan, 1.0);
    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert!(result.ir().model.bodies[0]
        .id
        .to_string()
        .contains("definition-member-3"));
    let definition_loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("instance-definition record"))
        .expect("aggregated definition diagnostic");
    assert_eq!(
        definition_loss
            .provenance
            .as_ref()
            .and_then(|value| value.tag.as_deref()),
        Some("INSTANCE_DEFINITION_TABLE")
    );
}

#[test]
pub(crate) fn static_instance_suppresses_member_and_two_references_expand_with_distinct_ids() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x51; 16];
    let definition_id = [0x61; 16];
    let first_reference_id = [0x71; 16];
    let second_reference_id = [0x72; 16];
    let member =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let first = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let second = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [20.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[member, first, second]);
    set_identity(&mut scan, 0, member_id, "member", None, true);
    set_identity(&mut scan, 1, first_reference_id, "first", None, true);
    set_identity(&mut scan, 2, second_reference_id, "second", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.bodies.len(), 2);
    assert_eq!(result.ir().model.points.len(), 2);
    assert_eq!(
        result
            .ir()
            .model
            .bodies
            .iter()
            .map(|body| body.transform.expect("required invariant").rows()[0][3])
            .collect::<Vec<_>>(),
        vec![10.0, 20.0]
    );
    let body_ids = result
        .ir()
        .model
        .bodies
        .iter()
        .map(|body| body.id.to_string())
        .collect::<Vec<_>>();
    assert_ne!(body_ids[0], body_ids[1]);
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links,
        body_ids
    );
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[1]
            .links
            .len(),
        1
    );
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[2]
            .links
            .len(),
        1
    );
    let native = result
        .ir()
        .native
        .namespace("rhino")
        .expect("required invariant");
    assert_eq!(native.arenas["product_definitions"].len(), 1);
    assert_eq!(native.arenas["product_occurrences"].len(), 2);
    assert_eq!(
        native.arenas["product_occurrences"][0].fields()["definition_uuid"],
        Uuid::from_wire(definition_id).to_string()
    );
    assert_eq!(
        native.arenas["product_occurrences"][0].fields()["transform_units"],
        "millimeter"
    );
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::RhinoLossCode::ObjectRecordCensus.kind()
        && loss.message.contains("decoded 3/3 Rhino object records")));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn instance_transform_uses_member_carriers_for_mixed_body_and_free_geometry() {
    let archive = ArchiveVersion::V5;
    let point_id = [0x58; 16];
    let curve_id = [0x59; 16];
    let definition_id = [0x68; 16];
    let reference_id = [0x79; 16];
    let point =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 2.0, 3.0]));
    let curve = object_record_with_payload(
        archive,
        4,
        NURBS_CURVE_CLASS,
        &nurbs_curve_payload([[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]),
    );
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[point, curve, reference]);
    set_identity(&mut scan, 0, point_id, "point-member", None, true);
    set_identity(&mut scan, 1, curve_id, "curve-member", None, true);
    set_identity(&mut scan, 2, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[point_id, curve_id])],
    );

    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.points[0].position.x, 11.0);
    assert_eq!(
        result.ir().model.bodies[0]
            .transform
            .expect("body carrier transform")
            .rows()[0][3],
        10.0
    );
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(curve) = &result.ir().model.curves[0].geometry
    else {
        panic!("free member curve must remain a transformed solved carrier");
    };
    assert_eq!(curve.control_points()[0].x, 11.0);
    assert_eq!(curve.control_points()[1].x, 12.0);
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
pub(crate) fn nested_instance_composes_parent_child_and_records_outer_to_inner_path() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x52; 16];
    let nested_reference_id = [0x73; 16];
    let world_reference_id = [0x74; 16];
    let inner_definition_id = [0x62; 16];
    let outer_definition_id = [0x63; 16];
    let curve = object_record_with_payload(
        archive,
        4,
        NURBS_CURVE_CLASS,
        &nurbs_curve_payload([[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]),
    );
    let nested = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(inner_definition_id, transform(2.0, [0.0, 0.0, 0.0])),
    );
    let world = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(outer_definition_id, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[curve, nested, world]);
    set_identity(&mut scan, 0, member_id, "curve", None, true);
    set_identity(
        &mut scan,
        1,
        nested_reference_id,
        "nested-reference",
        None,
        true,
    );
    set_identity(
        &mut scan,
        2,
        world_reference_id,
        "world-reference",
        Some([255, 0, 0, 0]),
        false,
    );
    install_definitions(
        &mut scan,
        vec![
            static_definition(inner_definition_id, &[member_id]),
            static_definition(outer_definition_id, &[nested_reference_id]),
        ],
    );

    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.curves.len(), 1);
    let curve = &result.ir().model.curves[0];
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("expected transformed NURBS");
    };
    assert_eq!(nurbs.control_points()[0].x, 12.0);
    assert_eq!(nurbs.control_points()[1].x, 14.0);
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("required invariant")
            .instance_path,
        vec![
            Uuid::from_wire(world_reference_id).to_string(),
            Uuid::from_wire(nested_reference_id).to_string()
        ]
    );
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("required invariant")
            .color,
        Some(cadmpeg_ir::topology::Color {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        })
    );
    assert_eq!(
        curve
            .source_object
            .as_ref()
            .expect("required invariant")
            .visible,
        Some(false)
    );
    assert!(curve.id.to_string().contains(&format!(
        "{}.{}",
        Uuid::from_wire(world_reference_id),
        Uuid::from_wire(nested_reference_id)
    )));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
pub(crate) fn nil_and_duplicate_reference_ids_use_distinct_record_path_segments() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x53; 16];
    let definition_id = [0x64; 16];
    let duplicate_reference_id = [0x75; 16];
    let curve = object_record_with_payload(
        archive,
        4,
        NURBS_CURVE_CLASS,
        &nurbs_curve_payload([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
    );
    let reference = || {
        object_record_with_payload(
            archive,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &instance_reference_payload(definition_id, transform(1.0, [0.0, 0.0, 0.0])),
        )
    };
    let mut scan = scan_with_objects(&[curve, reference(), reference(), reference(), reference()]);
    set_identity(&mut scan, 0, member_id, "member", None, true);
    set_identity(&mut scan, 1, [0; 16], "nil-first", None, true);
    set_identity(&mut scan, 2, [0; 16], "nil-second", None, true);
    set_identity(
        &mut scan,
        3,
        duplicate_reference_id,
        "duplicate-first",
        None,
        true,
    );
    set_identity(
        &mut scan,
        4,
        duplicate_reference_id,
        "duplicate-second",
        None,
        true,
    );
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.curves.len(), 4);
    let ids = result
        .ir()
        .model
        .curves
        .iter()
        .map(|curve| curve.id.to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 4);
    let paths = result
        .ir()
        .model
        .curves
        .iter()
        .map(|curve| {
            curve
                .source_object
                .as_ref()
                .expect("required invariant")
                .instance_path
                .clone()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(paths.len(), 4);
    assert!(paths
        .iter()
        .flatten()
        .all(|segment| segment.starts_with("record-")));
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[0]
            .links
            .len(),
        4
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
pub(crate) fn instance_bakes_mesh_subd_and_normals_without_changing_subd_metadata() {
    let archive = ArchiveVersion::V5;
    let mesh_id = [0x54; 16];
    let subd_id = [0x55; 16];
    let definition_id = [0x65; 16];
    let reference_id = [0x76; 16];
    let mesh = object_record_with_payload(archive, 0x20, MESH_CLASS, &mesh_payload());
    let subd = object_record_with_payload(
        archive,
        0x0004_0000,
        SUBD_CLASS,
        &crate::subd::tests::quad_payload(archive),
    );
    let rows = [
        [2.0, 0.0, 0.0, 5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.5, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, rows),
    );
    let mut scan = scan_with_objects(&[mesh, subd, reference]);
    set_identity(&mut scan, 0, mesh_id, "mesh", None, true);
    set_identity(&mut scan, 1, subd_id, "subd", None, true);
    set_identity(&mut scan, 2, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[mesh_id, subd_id])],
    );

    let result = crate::decode::decode_for_test(&scan);
    let mesh = &result.ir().model.tessellations[0];
    assert_eq!(mesh.vertices()[0].x, 5.0);
    assert_eq!(mesh.vertices()[1].x, 7.0);
    assert_eq!(
        mesh.normals()[0],
        cadmpeg_ir::math::Vector3::new(0.242_535_625_036_332_97, 0.0, 0.970_142_500_145_331_9)
    );
    let subd = &result.ir().model.subds[0];
    assert_eq!(subd.vertices[2].point.x, 7.0);
    assert_eq!(subd.edges[0].sharpness, [0.25, 0.25]);
    assert_eq!(subd.edges[0].sector_coefficients, [0.125, 0.875]);
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn failed_instance_expansion_retains_inflated_member_mesh_budget() {
    let archive = ArchiveVersion::V5;
    let mesh_id = [0x54; 16];
    let missing_id = [0x99; 16];
    let definition_id = [0x65; 16];
    let reference_id = [0x76; 16];
    let mesh = object_record_with_payload(
        archive,
        0x20,
        MESH_CLASS,
        &crate::test_support::mesh_payload(3, 0, false, false),
    );
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(1.0, [0.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[mesh, reference]);
    set_identity(&mut scan, 0, mesh_id, "mesh", None, true);
    set_identity(&mut scan, 1, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[mesh_id, missing_id])],
    );

    crate::decode::with_expand(&scan, |expand| {
        let mut context = crate::decode::DecodeContext::new(&scan, expand);
        context.decode_geometry();
        assert!(context.mesh_budget_used() > 0);
        let result = crate::decode::seal_for_test(context.commit(), false);
        assert!(result.ir().model.tessellations.is_empty());
        assert!(result.ir().model.bodies.is_empty());
    });
}

#[test]
pub(crate) fn nonuniform_instance_converts_analytic_circle_to_exact_nurbs() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x56; 16];
    let definition_id = [0x66; 16];
    let reference_id = [0x77; 16];
    let circle = object_record_with_payload(archive, 4, ARC_CURVE_CLASS, &circle_payload());
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(2.0, [0.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[circle, reference]);
    set_identity(&mut scan, 0, member_id, "circle", None, true);
    set_identity(&mut scan, 1, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = crate::decode::decode_for_test(&scan);
    let cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) = &result.ir().model.curves[0].geometry
    else {
        panic!("nonuniform circle must become NURBS");
    };
    assert_eq!(nurbs.degree(), 2);
    assert_eq!(nurbs.control_points()[0].x, 2.0);
    assert_eq!(nurbs.control_points()[2].y, 1.0);
    assert_eq!(
        nurbs.weights().expect("required invariant")[1],
        std::f64::consts::FRAC_1_SQRT_2
    );
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
pub(crate) fn transformed_procedural_instance_keeps_solved_carriers_without_dangling_references() {
    let archive = ArchiveVersion::V5;
    let member_id = [0x57; 16];
    let definition_id = [0x67; 16];
    let reference_id = [0x78; 16];
    let revolution = object_record_with_payload(
        archive,
        8,
        REV_SURFACE_CLASS,
        &crate::surfaces::tests::valid_revolution_payload(0x20),
    );
    let reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(definition_id, transform(2.0, [3.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[revolution, reference]);
    set_identity(&mut scan, 0, member_id, "revolution", None, true);
    set_identity(&mut scan, 1, reference_id, "reference", None, true);
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );

    let result = crate::decode::decode_for_test(&scan);
    assert!(!result.ir().model.surfaces.is_empty());
    assert!(result.ir().model.procedural_surfaces.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("exact solved carrier retained")));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}

#[test]
fn branching_instance_budget_retains_current_reference_and_later_reference_recovers() {
    let archive = ArchiveVersion::V5;
    let first_member_id = [0x31; 16];
    let second_member_id = [0x32; 16];
    let wide_definition = [0x41; 16];
    let narrow_definition = [0x42; 16];
    let first_member =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 0.0, 0.0]));
    let second_member =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([2.0, 0.0, 0.0]));
    let wide_reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(wide_definition, transform(1.0, [0.0, 0.0, 0.0])),
    );
    let narrow_reference = object_record_with_payload(
        archive,
        0x1000,
        INSTANCE_REFERENCE_CLASS,
        &instance_reference_payload(narrow_definition, transform(1.0, [10.0, 0.0, 0.0])),
    );
    let mut scan = scan_with_objects(&[
        first_member,
        second_member,
        wide_reference,
        narrow_reference,
    ]);
    for (source_order, id) in [first_member_id, second_member_id, [0x51; 16], [0x52; 16]]
        .into_iter()
        .enumerate()
    {
        set_identity(
            &mut scan,
            source_order,
            id,
            &format!("budget-{source_order}"),
            None,
            true,
        );
    }
    install_definitions(
        &mut scan,
        vec![
            static_definition(wide_definition, &[first_member_id, second_member_id]),
            static_definition(narrow_definition, &[second_member_id]),
        ],
    );
    crate::decode::with_expand(&scan, |expand| {
        let mut context = crate::decode::DecodeContext::new(&scan, expand);
        context.set_expansion_limits([16, 1, 128]);
        context.decode_geometry();
        let result = crate::decode::seal_for_test(context.commit(), false);
        assert_eq!(result.ir().model.points.len(), 1);
        assert_eq!(
            result.ir().model.bodies[0]
                .transform
                .expect("instance transform")
                .rows()[0][3],
            10.0
        );
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("instance member budget exceeded")));
    });
}

#[test]
fn invalid_instance_families_are_atomic_and_later_reference_recovers() {
    let archive = ArchiveVersion::V5;
    let nested_b_id = [0x81; 16];
    let nested_a_id = [0x82; 16];
    let ambiguous_member_id = [0x83; 16];
    let valid_member_id = [0x84; 16];
    let unknown_member_id = [0x85; 16];
    let definition_a = [0x91; 16];
    let definition_b = [0x92; 16];
    let missing_member_definition = [0x93; 16];
    let duplicate_member_definition = [0x94; 16];
    let ambiguous_member_definition = [0x95; 16];
    let external_definition = [0x96; 16];
    let valid_definition = [0x97; 16];
    let unknown_definition = [0x98; 16];
    let missing_definition = [0x99; 16];

    let reference_object = |definition, rows| {
        object_record_with_payload(
            archive,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &instance_reference_payload(definition, rows),
        )
    };
    let nested_b = reference_object(definition_b, transform(1.0, [0.0, 0.0, 0.0]));
    let nested_a = reference_object(definition_a, transform(1.0, [0.0, 0.0, 0.0]));
    let ambiguous_first =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([1.0, 0.0, 0.0]));
    let ambiguous_second =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([2.0, 0.0, 0.0]));
    let valid_member =
        object_record_with_payload(archive, 1, POINT_CLASS, &point_payload([3.0, 0.0, 0.0]));
    let unknown_member = object_record_with_payload(archive, 8, REV_SURFACE_CLASS, &[0]);
    let world_cycle = reference_object(definition_a, transform(1.0, [0.0, 0.0, 0.0]));
    let missing_definition_reference =
        reference_object(missing_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let missing_member_reference =
        reference_object(missing_member_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let duplicate_member_reference =
        reference_object(duplicate_member_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let ambiguous_member_reference =
        reference_object(ambiguous_member_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let external_reference = reference_object(external_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let mut singular = transform(1.0, [0.0, 0.0, 0.0]);
    singular[2][2] = 0.0;
    let singular_reference = reference_object(valid_definition, singular);
    let mut nonfinite = transform(1.0, [0.0, 0.0, 0.0]);
    nonfinite[0][0] = f64::NAN;
    let nonfinite_reference = reference_object(valid_definition, nonfinite);
    let unknown_reference = reference_object(unknown_definition, transform(1.0, [0.0, 0.0, 0.0]));
    let valid_reference = reference_object(valid_definition, transform(1.0, [30.0, 0.0, 0.0]));

    let mut scan = scan_with_objects(&[
        nested_b,
        nested_a,
        ambiguous_first,
        ambiguous_second,
        valid_member,
        unknown_member,
        world_cycle,
        missing_definition_reference,
        missing_member_reference,
        duplicate_member_reference,
        ambiguous_member_reference,
        external_reference,
        singular_reference,
        nonfinite_reference,
        unknown_reference,
        valid_reference,
    ]);
    let identities = [
        nested_b_id,
        nested_a_id,
        ambiguous_member_id,
        ambiguous_member_id,
        valid_member_id,
        unknown_member_id,
        [0xa0; 16],
        [0xa1; 16],
        [0xa2; 16],
        [0xa3; 16],
        [0xa4; 16],
        [0xa5; 16],
        [0xa6; 16],
        [0xa7; 16],
        [0xa8; 16],
        [0xa9; 16],
    ];
    for (source_order, id) in identities.into_iter().enumerate() {
        set_identity(
            &mut scan,
            source_order,
            id,
            &format!("object-{source_order}"),
            None,
            true,
        );
    }
    let missing_member_id = [0xff; 16];
    let mut external = static_definition(external_definition, &[]);
    external.kind = crate::instances::DefinitionKind::Linked;
    install_definitions(
        &mut scan,
        vec![
            static_definition(definition_a, &[nested_b_id]),
            static_definition(definition_b, &[nested_a_id]),
            static_definition(missing_member_definition, &[missing_member_id]),
            static_definition(
                duplicate_member_definition,
                &[valid_member_id, valid_member_id],
            ),
            static_definition(ambiguous_member_definition, &[ambiguous_member_id]),
            external,
            static_definition(valid_definition, &[valid_member_id]),
            static_definition(unknown_definition, &[unknown_member_id]),
        ],
    );

    let result = crate::decode::decode_for_test(&scan);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.points.len(), 1);
    assert!(result.ir().model.surfaces.is_empty());
    assert_eq!(
        result.ir().model.bodies[0]
            .transform
            .expect("required invariant")
            .rows()[0][3],
        30.0
    );
    for unknown in &result
        .ir()
        .native_unknowns("rhino")
        .expect("required invariant")[6..15]
    {
        assert!(unknown.links.is_empty());
    }
    assert_eq!(
        result
            .ir()
            .native_unknowns("rhino")
            .expect("required invariant")[15]
            .links
            .len(),
        1
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == crate::loss::RhinoLossCode::ObjectDecodeDiagnostic.kind()
            && loss
                .message
                .contains("f9cfb638-b9d4-4340-87e3-c56e7865d96a:")
            && loss.message.contains("decode warnings")
    }));
    assert!(cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).is_ok());
}
