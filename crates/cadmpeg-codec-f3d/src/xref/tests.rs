// SPDX-License-Identifier: Apache-2.0
//! XREF and BREP-less document tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::collections::HashSet;
use std::io::{Cursor, Read, Seek, Write};

use cadmpeg_asm::asm_header;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, Encoder};
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
use cadmpeg_ir::report::{LossKind as LossCode, LossTaxonomy, Severity};
use zip::CompressionMethod;

use crate::bytes::lp_utf16_bytes;
use crate::container::{self, role};
use crate::loss::F3dLossCode;
use crate::test_support::*;
use crate::F3dCodec;

use super::*;

#[test]
fn redirections_keep_neutron_role_and_data_independent() {
    let table = super::parse(
        br#"{"designs":[],"references":[{"from":"root.f3d","relativePath":"part.f3d","type":"XREF","properties":[{"neutronRole":{"value":"role-guid","dataType":"STRING"}},{"neutronData":{"value":"data-guid","dataType":"STRING"}}]}]}"#,
    )
    .expect("redirections JSON");
    assert_eq!(table.references.len(), 1);
    assert_eq!(table.references[0].neutron_role, "role-guid");
    assert_eq!(table.references[0].neutron_data, "data-guid");
}

#[test]
fn external_reference_placements_project_as_root_occurrences_in_millimetres() {
    let transform = [
        [0.0, -1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 2.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let table = super::XrefTable {
        designs: Vec::new(),
        references: vec![crate::records::XrefReference {
            id: "f3d:xref:reference#0-occurrence-0".into(),
            ordinal: 0,
            occurrence_ordinal: 0,
            from: "root.f3d".into(),
            relative_path: "part.f3d".into(),
            neutron_role: "role".into(),
            neutron_data: "data".into(),
            transform: Some(transform),
        }],
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
    };

    let occurrences = super::project_occurrences(&table);

    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].id.0, "f3d:model:occurrence#xref-0-0");
    assert_eq!(
        occurrences[0].transform.rows,
        [
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 30.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    assert_eq!(
        occurrences[0].parent,
        cadmpeg_ir::products::OccurrenceParent::Root
    );
    assert_eq!(
        occurrences[0].prototype,
        cadmpeg_ir::products::PrototypeReference::External {
            document: cadmpeg_ir::products::ExternalDocumentReference {
                path: Some("part.f3d".into()),
                document_id: None,
                resolution: cadmpeg_ir::products::ExternalResolution::Unresolved,
            },
            object: None,
        }
    );
}

#[test]
fn component_reference_data_is_an_open_json_object() {
    let value = super::parse_component_reference_data(
        br#"{"schema":7,"references":[{"id":"component"}],"extension":{"x":true}}"#,
    )
    .expect("open component-reference object");
    assert_eq!(value["schema"], 7);
    assert!(super::parse_component_reference_data(br"[]").is_err());
    assert!(super::parse_component_reference_data(b"not-json").is_err());
}

fn local_reference(target: u64) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn cross_document_reference(target: u64, link_name: &str) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(
        "11111111-2222-3333-4444-555555555555",
    ));
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    bytes.extend(crate::bytes::lp_utf16_bytes(link_name));
    bytes.push(0);
    bytes
}

/// One occurrence-placement record: a target path whose last element
/// carries `role` as its cross-document link name, the identity marker,
/// and the three closing reference runs.
fn occurrence_record(
    role: &str,
    entity_id: u64,
    discriminators: &[u32],
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    occurrence_record_with_serializer_magic(role, entity_id, discriminators, transform, None)
}

fn occurrence_record_with_serializer_magic(
    role: &str,
    entity_id: u64,
    discriminators: &[u32],
    transform: Option<[[f64; 4]; 4]>,
    serializer_magic: Option<u32>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&(discriminators.len() as u32).to_le_bytes());
    for (ordinal, discriminator) in discriminators.iter().enumerate() {
        let target = 100 + ordinal as u64;
        if ordinal + 1 == discriminators.len() {
            bytes.extend(cross_document_reference(target, role));
        } else {
            bytes.extend(local_reference(target));
        }
        bytes.extend_from_slice(&discriminator.to_le_bytes());
    }
    bytes.push(0);
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(7));
    if serializer_magic == Some(crate::metastream::MODERN_SERIALIZER_MAGIC) {
        bytes.push(2);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&42_u32.to_le_bytes());
        bytes.extend(local_reference(8));
    }
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

fn repeated_target_occurrence_record(
    role: &str,
    entity_id: u64,
    envelope_discriminator: u32,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    repeated_target_occurrence_record_with_path_role(
        role,
        role,
        entity_id,
        envelope_discriminator,
        transform,
    )
}

fn repeated_target_occurrence_record_with_path_role(
    path_role: &str,
    role: &str,
    entity_id: u64,
    envelope_discriminator: u32,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let mut bytes = occurrence_record(path_role, entity_id, &[1], None);
    let path_end = super::occurrence_path(&bytes).expect("synthetic path").2;
    bytes.truncate(path_end);
    bytes.extend_from_slice(&envelope_discriminator.to_le_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(metadata_guid_a));
    bytes.extend(crate::bytes::lp_utf16_bytes(metadata_guid_b));
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    bytes.extend(crate::bytes::lp_utf16_bytes(component_guid));
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(role));
    bytes.push(0);
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(role));
    bytes.push(0);
    bytes.extend(local_reference(3));
    bytes
}

#[test]
fn repeated_target_placements_decode_identity_and_matrix_forms() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let identity = repeated_target_occurrence_record(role, 10, 1, None);
    let matrix_record = repeated_target_occurrence_record(role, 11, 5, Some(matrix));
    assert_eq!(identity.len(), 695);
    assert_eq!(matrix_record.len(), 823);
    let mut bytes = identity;
    bytes.extend(matrix_record);

    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].discriminators, vec![1]);
    assert_eq!(placements[1].discriminators, vec![1]);
    assert_eq!(
        super::occurrence_transforms(&placements, role),
        vec![None, Some(matrix)]
    );

    let retained_role = format!("{role}_urn:example:component");
    let path_role = "11112222-3333-4444-5555-666677778888_urn:example:path-component";
    let local_carrier =
        repeated_target_occurrence_record_with_path_role(path_role, &retained_role, 12, 1, None);
    let (decoded_role, role_offset) = super::repeated_target_component_insert_identity(
        &local_carrier,
        0,
        local_carrier.len(),
        12,
    )
    .expect("identity carrier with an independent retained role");
    assert_eq!(decoded_role, retained_role);
    let encoded_role = crate::bytes::lp_utf16_bytes(&retained_role);
    assert_eq!(
        &local_carrier[role_offset - 4..role_offset - 4 + encoded_role.len()],
        encoded_role
    );
}

fn grouped_identity_carrier(role: &str, record_index: u32) -> Vec<u8> {
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"382");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend(crate::bytes::lp_utf16_bytes(component_guid));
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(role));
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    bytes.extend(crate::bytes::lp_utf16_bytes(metadata_guid_a));
    bytes.extend(crate::bytes::lp_utf16_bytes(metadata_guid_b));
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    bytes.extend(crate::bytes::lp_utf16_bytes(component_guid));
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(type_guid.as_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(role));
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    bytes.extend(crate::bytes::lp_utf16_bytes(role));
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);
    bytes
}

#[test]
fn grouped_identity_carriers_decode_as_identity_placements() {
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let bytes = grouped_identity_carrier(role, 10);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(
        placements,
        vec![OccurrencePlacement {
            link_names: vec![role.into()],
            discriminators: vec![1],
            transform: None,
        }]
    );
}

fn legacy_occurrence_reference(target: u64, identity: u64) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&identity.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    bytes.push(0);
    bytes
}

/// One legacy typed placement envelope. The target-reference fields are
/// deliberately built from independent values; only the role and transform
/// are projected by the placement reader.
fn legacy_occurrence_record(
    role: &str,
    entity_id: u64,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(legacy_occurrence_reference(3, 0x0102_0304_0506_0708));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(
        "11111111-2222-3333-4444-555555555555",
    ));
    bytes.extend(crate::bytes::lp_utf16_bytes(
        "66666666-7777-8888-9999-aaaaaaaaaaaa",
    ));
    bytes.push(0);
    bytes.extend(legacy_occurrence_reference(3, 0x1112_1314_1516_1718));
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(role));
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

#[test]
fn legacy_typed_placements_decode_identity_and_matrix_forms() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let identity = legacy_occurrence_record(role, 10, None);
    let matrix_record = legacy_occurrence_record(role, 11, Some(matrix));
    assert_eq!(identity.len(), 403);
    assert_eq!(matrix_record.len(), 531);
    let mut bytes = identity;
    bytes.extend(matrix_record);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0].discriminators, vec![1]);
    assert_eq!(placements[1].discriminators, vec![1]);
    assert_eq!(
        super::occurrence_transforms(&placements, role),
        vec![None, Some(matrix)]
    );
}

#[test]
fn malformed_legacy_typed_placement_reports_its_role() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let mut bytes = legacy_occurrence_record(role, 10, None);
    bytes.pop();
    let (placements, failures) = super::occurrence_placements_with_failures(
        &bytes,
        &super::indexed_records(&bytes),
        None,
        None,
    );

    assert!(placements.is_empty());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].link_names, vec![role]);
}

#[test]
fn occurrence_records_expand_shared_roles_and_decode_rigid_matrices() {
    let first = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let second = [
        [1.0, 0.0, 0.0, -5.0],
        [0.0, 1.0, 0.0, 6.0],
        [0.0, 0.0, 1.0, 7.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut bytes = occurrence_record("role", 10, &[1], Some(first));
    bytes.extend_from_slice(&occurrence_record("role", 11, &[1, 2], Some(second)));
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(
        super::occurrence_transforms(&placements, "role"),
        vec![Some(first), Some(second)]
    );
}

#[test]
fn identity_marked_placement_stores_no_matrix() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.extend_from_slice(&occurrence_record("role", 11, &[3], None));
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(placements.len(), 2);
    assert_eq!(
        super::occurrence_transforms(&placements, "role"),
        vec![None, None]
    );
}

#[test]
fn malformed_role_placement_is_retained_as_a_decode_failure() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.pop();
    let records = super::indexed_records(&bytes);

    let (placements, failures) =
        super::occurrence_placements_with_failures(&bytes, &records, None, None);

    assert!(placements.is_empty());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].link_names, vec!["role"]);
}

#[test]
fn tagged_placement_tail_requires_the_modern_serializer_magic() {
    let modern = occurrence_record_with_serializer_magic(
        "role",
        10,
        &[1],
        None,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    let records = super::indexed_records(&modern);
    assert_eq!(
        super::occurrence_placements(
            &modern,
            &records,
            Some(crate::metastream::MODERN_SERIALIZER_MAGIC)
        )
        .len(),
        1
    );
    assert!(
        super::occurrence_placements(&modern, &records, Some(999)).is_empty(),
        "a legacy MetaStream must not admit the modern tagged tail"
    );

    let legacy = occurrence_record("role", 11, &[2], None);
    let records = super::indexed_records(&legacy);
    assert_eq!(
        super::occurrence_placements(&legacy, &records, Some(999)).len(),
        1
    );
    assert!(
        super::occurrence_placements(
            &legacy,
            &records,
            Some(crate::metastream::MODERN_SERIALIZER_MAGIC)
        )
        .is_empty(),
        "the modern form requires its tagged tail"
    );
}

#[test]
fn paired_design_metastream_selects_the_tagged_placement_form() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let placement = occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        Some([
            [1.0, 0.0, 0.0, 7.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    let mut placement = placement;
    placement[4..7].copy_from_slice(b"256");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(&(properties.len() as u32).to_le_bytes())
        .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic XRef archive");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    let transform = native.xref_references[0]
        .transform
        .expect("tagged placement transform");
    assert!((transform[0][3] - 7.0).abs() < 1e-12);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::XrefPlacementUndecoded.kind()));
}

#[test]
fn paired_design_metastream_selects_the_legacy_typed_placement_form() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let placement = legacy_occurrence_record(
        role,
        10,
        Some([
            [1.0, 0.0, 0.0, 7.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
    );
    let mut placement = placement;
    placement[4..7].copy_from_slice(b"256");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(&(properties.len() as u32).to_le_bytes())
        .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic legacy XRef archive");
    let native = f3d_native(decoded.ir());
    assert_eq!(native.xref_references.len(), 1);
    let transform = native.xref_references[0]
        .transform
        .expect("legacy placement transform");
    assert!((transform[0][3] - 7.0).abs() < 1e-12);
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code != F3dLossCode::XrefPlacementUndecoded.kind()));
}

#[test]
fn malformed_typed_role_placement_reports_a_loss() {
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let properties =
        br#"{"docstruct":{"version":"1.0.0","type":"assembly-design","subtype":"synthetic","attributes":{}}}"#;
    let mut placement = occurrence_record_with_serializer_magic(
        role,
        10,
        &[1],
        None,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    );
    placement.pop();
    placement[4..7].copy_from_slice(b"256");

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("Properties.dat", stored).unwrap();
    zip.write_all(&(properties.len() as u32).to_le_bytes())
        .unwrap();
    zip.write_all(properties).unwrap();
    zip.start_file("RedirectionsStream.dat", stored).unwrap();
    zip.write_all(redirections_json("root.f3d", &[("part.f3d", role)]).as_bytes())
        .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&design_metastream_with_records(
        &[(
            super::OCCURRENCE_PLACEMENT_TYPE_GUID,
            "",
            2,
            "Component",
            &[10],
        )],
        &[(10, 0)],
    ))
    .unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&placement).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("synthetic malformed placement archive");
    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::XrefPlacementUndecoded.kind())
        .expect("typed placement loss");
    assert!(loss.message.contains("part.f3d"));
    assert!(loss.message.contains(role));
}

#[test]
fn placement_keeps_the_instance_discriminator_of_every_path_element() {
    let bytes = occurrence_record("role", 10, &[7, 4, 2], None);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(placements[0].discriminators, vec![7, 4, 2]);
    assert_eq!(placements[0].link_names, vec!["role".to_owned()]);
}

#[test]
fn a_placement_that_does_not_close_on_the_record_end_is_not_a_placement() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.push(0);
    let records = super::indexed_records(&bytes);

    assert_eq!(
        super::occurrence_placements(&bytes, &records, None),
        Vec::new()
    );
}

#[test]
fn a_nonrigid_matrix_is_not_a_placement() {
    let mut nonrigid = [[0.0; 4]; 4];
    nonrigid[0][0] = 2.0;
    nonrigid[1][1] = 1.0;
    nonrigid[2][2] = 1.0;
    nonrigid[3][3] = 1.0;
    let bytes = occurrence_record("role", 10, &[1], Some(nonrigid));
    let records = super::indexed_records(&bytes);

    assert_eq!(
        super::occurrence_placements(&bytes, &records, None),
        Vec::new()
    );
}

#[test]
fn a_role_that_no_path_element_names_places_nothing() {
    let bytes = occurrence_record("role", 10, &[1], None);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes), None);

    assert_eq!(
        super::occurrence_transforms(&placements, "other"),
        Vec::new()
    );
}

#[test]
fn exact_component_insert_carriers_precede_structured_placements() {
    let direct = [
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let structured = OccurrencePlacement {
        link_names: vec!["role".into()],
        discriminators: vec![1],
        transform: Some([
            [1.0, 0.0, 0.0, -5.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]),
    };

    assert_eq!(
        super::occurrence_transforms_with_precedence(vec![direct], &[structured.clone()], "role"),
        vec![Some(direct)]
    );
    assert_eq!(
        super::superseded_placement_count(
            std::slice::from_ref(&direct),
            std::slice::from_ref(&structured),
            "role"
        ),
        1
    );
}

#[test]
fn component_insert_selection_uses_stream_and_role_not_class_tag() {
    let selected = [
        [1.0, 0.0, 0.0, 7.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let ignored = [
        [1.0, 0.0, 0.0, 9.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let selected_construction = crate::records::DesignComponentInsertConstruction {
        relation_record_index: 1,
        carrier_record_index: 2,
        occurrence_identity: None,
        neutron_role: "role".into(),
        neutron_role_offset: 0,
        transform: selected,
        transform_offset: Some(0),
        carrier_transform_offset: Some(0),
    };
    let ignored_construction = crate::records::DesignComponentInsertConstruction {
        neutron_role: "other".into(),
        transform: ignored,
        ..selected_construction.clone()
    };

    assert_eq!(
        super::select_component_insert_transforms(
            [
                ("stream", &selected_construction),
                ("stream", &ignored_construction),
                ("other-stream", &selected_construction),
            ],
            "stream",
            "role"
        ),
        vec![selected]
    );
}

#[test]
fn typed_placement_admission_rejects_shape_collision() {
    let bytes = occurrence_record("role", 10, &[2, 3], None);
    let records = super::indexed_records(&bytes);
    let no_registered_placements = HashSet::new();
    assert!(super::occurrence_placements_filtered(
        &bytes,
        &records,
        None,
        Some(&no_registered_placements),
    )
    .is_empty());

    let registered_placement = HashSet::from([0]);
    assert_eq!(
        super::occurrence_placements_filtered(&bytes, &records, None, Some(&registered_placement),)
            .len(),
        1
    );
}

#[test]
fn assembly_root_without_brep_is_not_a_blocking_loss() {
    let archive = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .all(|loss| loss.severity < cadmpeg_ir::report::Severity::Error),
        "assembly document must not report blocking/error losses: {:?}",
        decoded.report().losses
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("assembly document")));
    assert!(decoded
        .report()
        .notes
        .iter()
        .any(|note| note.contains("comp.f3d") && note.contains(XREF_ROLE)));
    let native =
        crate::native::F3dNative::load(decoded.ir().native.namespace("f3d").unwrap()).unwrap();
    assert_eq!(native.xref_designs.len(), 2);
    assert_eq!(native.xref_references.len(), 1);
    assert_eq!(native.xref_references[0].relative_path, "comp.f3d");
    assert_eq!(native.xref_references[0].neutron_role, XREF_ROLE);
    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(
        source.attributes.get("docstruct_type").map(String::as_str),
        Some("assembly-design")
    );
}

#[test]
fn part_without_brep_keeps_blocking_losses() {
    // A leaf redirections table (no outgoing references) does not make a
    // BREP-less part a valid assembly.
    let archive = f3d_without_brep("part-design", "part.f3d", &[]);
    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Blocking));
}

#[test]
fn redirections_leaf_form_parses_empty_object_references() {
    let table = crate::xref::parse(
        br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"part.f3d","displayName":"part","lineageUrn":"urn:l","versionUrn":"urn:v"}],"references":{}}"#,
    )
    .unwrap();
    assert_eq!(table.designs.len(), 1);
    assert_eq!(table.designs[0].target_file_name, "part.f3d");
    assert!(table.references.is_empty());
}
