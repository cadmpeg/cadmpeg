// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::Occurrence;

use super::{apply_hidden_occurrences, decode_stream, DesignHiddenOccurrence};
use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn header(out: &mut Vec<u8>, record_index: u32) {
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"412");
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn reference(out: &mut Vec<u8>, target: u32) {
    out.push(1);
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
}

/// A hidden-occurrence record: eight zero bytes, `u8 1`, the u32 count, the
/// marked references with two filler bytes between them, and a tail.
fn hidden_record(out: &mut Vec<u8>, record_index: u32, references: &[u32]) {
    header(out, record_index);
    out.extend_from_slice(&[0; 8]);
    out.push(1);
    out.extend_from_slice(&u32::try_from(references.len()).unwrap().to_le_bytes());
    for &target in references {
        reference(out, target);
        out.extend_from_slice(&[0, 0, 2, 0, 0, 0]);
    }
    out.extend_from_slice(&[0; 9]);
}

fn filler_record(out: &mut Vec<u8>, record_index: u32) {
    header(out, record_index);
    out.extend_from_slice(&[0; 20]);
}

fn set(values: &[u32]) -> BTreeSet<u32> {
    values.iter().copied().collect()
}

#[test]
fn a_record_hides_the_last_occurrence_it_names() {
    let mut bytes = Vec::new();
    filler_record(&mut bytes, 10);
    // Record 11 names component record 20, which is no occurrence, then
    // occurrence carrier 30 twice.
    hidden_record(&mut bytes, 11, &[20, 30, 30]);
    filler_record(&mut bytes, 12);
    let decoded = decode_stream(&bytes, STREAM, &set(&[11]), &set(&[30, 40]));
    assert_eq!(
        decoded,
        vec![DesignHiddenOccurrence {
            stream: STREAM.to_owned(),
            record_index: 11,
            target: 30,
            ancestors: Vec::new(),
        }]
    );
}

#[test]
fn earlier_occurrences_of_a_path_are_its_ancestors() {
    let mut bytes = Vec::new();
    hidden_record(&mut bytes, 11, &[40, 30, 40, 30]);
    let decoded = decode_stream(&bytes, STREAM, &set(&[11]), &set(&[30, 40]));
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].target, 30);
    assert_eq!(decoded[0].ancestors, vec![40]);
}

#[test]
fn records_outside_the_type_or_without_an_occurrence_are_not_hidden_records() {
    let mut bytes = Vec::new();
    hidden_record(&mut bytes, 11, &[20]);
    hidden_record(&mut bytes, 12, &[30]);
    // Record 13 stores a nonzero byte in the leading zero run.
    header(&mut bytes, 13);
    bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 7, 1, 1, 0, 0, 0]);
    reference(&mut bytes, 30);
    bytes.extend_from_slice(&[0; 9]);
    // Record 14 names occurrence 30 with a reference that lacks its four
    // zero bytes.
    header(&mut bytes, 14);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[1, 1, 0, 0, 0, 1, 30, 0, 0, 0, 9, 9, 9, 9]);
    let decoded = decode_stream(&bytes, STREAM, &set(&[11, 13, 14, 99]), &set(&[30]));
    assert!(decoded.is_empty());
    let decoded = decode_stream(&bytes, STREAM, &set(&[12]), &set(&[30]));
    assert_eq!(decoded.len(), 1);
}

fn guid(text: &str) -> crate::records::mesh::DesignRelaxedGuidText {
    text.to_owned().try_into().expect("GUID")
}

fn carrier(record_index: u32) -> DesignComponentOccurrence {
    DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
        id: format!("{STREAM}:design-component-occurrence#{record_index}"),
        class_tag: crate::records::references::DesignClassTag::try_from("339".to_owned()).unwrap(),
        record_index,
        byte_offset: u64::from(record_index),
        component_record_index: 159,
        component_guid: guid("2ad641b3-9ea3-4e5f-a4da-926c3faeccae"),
        occurrence_guid: guid("e1ac1a2e-15ee-4bb1-b9ab-1f1b3ec6ad54").into(),
        placement: DesignComponentOccurrencePlacement::Explicit {
            ordinal: std::num::NonZeroU32::new(1).unwrap(),
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
        },
        owner_entity: 6,
    })
    .unwrap()
}

fn occurrence_id(name: &str) -> OccurrenceId {
    OccurrenceId::mint(format!("f3d:model:occurrence#{name}")).unwrap()
}

/// The occurrence each named nested occurrence sits in.
fn parents(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<OccurrenceId, OccurrenceId> {
    pairs
        .iter()
        .map(|(child, parent)| (occurrence_id(child), occurrence_id(parent)))
        .collect()
}

fn occurrence(name: &str, carrier: &DesignComponentOccurrence) -> Occurrence {
    Occurrence {
        id: occurrence_id(name),
        prototype: cadmpeg_ir::ids::ProductDefinitionId::mint("f3d:model:component#definition")
            .unwrap(),
        transform: Some(cadmpeg_ir::transform::Transform::identity()),
        suppressed: None,
        visible: None,
        representation: None,
        overrides: Vec::new(),
        name: None,
        native_ref: Some(carrier.id.clone()),
    }
}

fn hidden(target: u32, ancestors: &[u32]) -> DesignHiddenOccurrence {
    DesignHiddenOccurrence {
        stream: STREAM.to_owned(),
        record_index: 1,
        target,
        ancestors: ancestors.to_vec(),
    }
}

#[test]
fn a_hidden_record_sets_visibility_on_each_instance_of_its_target() {
    let carriers = [carrier(30), carrier(40)];
    let mut occurrences = vec![occurrence("a", &carriers[0]), occurrence("b", &carriers[1])];
    apply_hidden_occurrences(
        &mut occurrences,
        &parents(&[]),
        &carriers,
        &[],
        &[hidden(30, &[])],
    );
    assert_eq!(occurrences[0].visible, Some(false));
    assert_eq!(occurrences[1].visible, None);
}

#[test]
fn a_named_ancestor_limits_the_hidden_instances_to_its_descendants() {
    let carriers = [carrier(30), carrier(40)];
    let mut occurrences = vec![
        occurrence("outer", &carriers[1]),
        occurrence("inside", &carriers[0]),
        occurrence("beside", &carriers[0]),
    ];
    apply_hidden_occurrences(
        &mut occurrences,
        &parents(&[("inside", "outer")]),
        &carriers,
        &[],
        &[hidden(30, &[40])],
    );
    assert_eq!(occurrences[0].visible, None);
    assert_eq!(occurrences[1].visible, Some(false));
    assert_eq!(occurrences[2].visible, None);
}

#[test]
fn a_record_of_another_stream_hides_nothing() {
    let carriers = [carrier(30)];
    let mut occurrences = vec![occurrence("a", &carriers[0])];
    let mut other = hidden(30, &[]);
    other.stream = "f3d:Other/BulkStream.dat".to_owned();
    apply_hidden_occurrences(&mut occurrences, &parents(&[]), &carriers, &[], &[other]);
    assert_eq!(occurrences[0].visible, None);
}

fn xref_reference(carrier_record_index: u64) -> crate::records::xref::XrefReference {
    crate::records::xref::XrefReference {
        id: "f3d:xref:reference#0-occurrence-0".into(),
        ordinal: 0,
        occurrence_ordinal: 0,
        placement_source: Some(crate::records::xref::XrefPlacementSource {
            frame_bytes: Vec::new().into(),
            occurrence_guid: None,
            stream: STREAM.into(),
            carrier_record_index,
            owner: None,
        }),
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "part.f3d".to_owned().try_into().unwrap(),
        neutron_role: "role".to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
        current_pose: None,
        operation_pose: None,
        contained_poses: Vec::new(),
        owned_poses: Vec::new(),
    }
}

#[test]
fn a_hidden_record_names_an_external_occurrence_by_its_placement_carrier() {
    let carriers = [carrier(30)];
    let references = [xref_reference(50)];
    let mut external = occurrence("external", &carriers[0]);
    external.native_ref = Some(references[0].id.clone());
    let mut occurrences = vec![occurrence("local", &carriers[0]), external];
    apply_hidden_occurrences(
        &mut occurrences,
        &parents(&[]),
        &carriers,
        &references,
        &[hidden(50, &[])],
    );
    assert_eq!(occurrences[0].visible, None);
    assert_eq!(occurrences[1].visible, Some(false));
    let mut bytes = Vec::new();
    hidden_record(&mut bytes, 11, &[50]);
    let decoded = decode_stream(&bytes, STREAM, &set(&[11]), &set(&[30, 50]));
    assert_eq!(decoded[0].target, 50);
}
