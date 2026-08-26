// SPDX-License-Identifier: Apache-2.0
//! XREF and BREP-less document tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::F3dCodec;

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
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

fn direct_occurrence_record(role: &str, transforms: &[[[f64; 4]; 4]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"382");
    bytes.extend_from_slice(&10_u64.to_le_bytes());
    for transform in transforms {
        bytes.extend_from_slice(&[0; 9]);
        let role = role.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(&(role.len() as u32).to_le_bytes());
        for value in role {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        for value in transform.iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
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
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes));

    assert_eq!(
        super::occurrence_transforms(&placements, "role"),
        vec![Some(first), Some(second)]
    );
}

#[test]
fn identity_marked_placement_stores_no_matrix() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.extend_from_slice(&occurrence_record("role", 11, &[3], None));
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes));

    assert_eq!(placements.len(), 2);
    assert_eq!(
        super::occurrence_transforms(&placements, "role"),
        vec![None, None]
    );
}

#[test]
fn placement_keeps_the_instance_discriminator_of_every_path_element() {
    let bytes = occurrence_record("role", 10, &[7, 4, 2], None);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes));

    assert_eq!(placements[0].discriminators, vec![7, 4, 2]);
    assert_eq!(placements[0].link_names, vec!["role".to_owned()]);
}

#[test]
fn a_placement_that_does_not_close_on_the_record_end_is_not_a_placement() {
    let mut bytes = occurrence_record("role", 10, &[1], None);
    bytes.push(0);
    let records = super::indexed_records(&bytes);

    assert_eq!(super::occurrence_placements(&bytes, &records), Vec::new());
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

    assert_eq!(super::occurrence_placements(&bytes, &records), Vec::new());
}

#[test]
fn a_role_that_no_path_element_names_places_nothing() {
    let bytes = occurrence_record("role", 10, &[1], None);
    let placements = super::occurrence_placements(&bytes, &super::indexed_records(&bytes));

    assert_eq!(
        super::occurrence_transforms(&placements, "other"),
        Vec::new()
    );
}

#[test]
fn repeated_roles_retain_each_directly_adjacent_occurrence_transform() {
    let first = [
        [1.0, 0.0, 0.0, -1.3],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let second = [
        [-1.0, 0.0, 0.0, -5.8],
        [0.0, 1.0, 0.0, 6.16],
        [0.0, 0.0, -1.0, 0.568],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bytes = direct_occurrence_record("role", &[first, second]);

    assert_eq!(
        super::role_adjacent_transforms(&bytes, &super::indexed_records(&bytes), "role"),
        [first, second]
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
