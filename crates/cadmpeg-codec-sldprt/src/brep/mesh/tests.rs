// SPDX-License-Identifier: Apache-2.0

use super::bodies;
use crate::brep::topology::nodes::sequence;
use crate::test_support::parasolid::mesh::mesh_body_sequence;
use crate::test_support::parasolid::sequence::topology_sequence;
use std::collections::HashSet;

#[test]
fn mesh_faces_count_distinct_owning_bodies() {
    for (body_count, face_count) in [(1, 1), (1, 2), (2, 1), (2, 2)] {
        let payload = mesh_body_sequence(body_count, face_count);
        let records = sequence(&payload).unwrap();
        let expected = (0..body_count)
            .map(|body| u32::from(body * 40 + 2))
            .collect();
        assert_eq!(bodies(&records), expected);
    }
}

#[test]
fn analytic_support_and_mesh_text_do_not_count() {
    let (mut payload, _) = topology_sequence();
    payload.extend(b"MESH Mesh mesh_body");
    assert_eq!(bodies(&sequence(&payload).unwrap()), HashSet::new());
}

#[test]
fn mesh_ownership_requires_each_typed_link() {
    for (kind, field, replacement) in [
        (201, "owner", 1u16),
        (201, "owner", 4),
        (14, "surface", 1),
        (14, "surface", 21),
        (14, "shell", 1),
        (14, "shell", 2),
        (13, "body", 1),
        (13, "body", 5),
    ] {
        let mut payload = mesh_body_sequence(1, 1);
        let records = sequence(&payload).unwrap();
        let node = records.nodes().find(|node| node.kind() == kind).unwrap();
        let range = node.field_range(field).unwrap();
        payload[range].copy_from_slice(&replacement.to_be_bytes());
        assert!(
            bodies(&sequence(&payload).unwrap()).is_empty(),
            "{kind} {field}"
        );
    }
}

#[test]
fn inactive_and_duplicate_body_identities_do_not_count() {
    for duplicate in [false, true] {
        let mut payload = mesh_body_sequence(1, 1);
        let records = sequence(&payload).unwrap();
        let body = records.nodes().find(|node| node.kind() == 12).unwrap();
        if duplicate {
            let fields = body.fields_start().unwrap()..body.range().end;
            let mut repeated = vec![0, 12, 0, 2];
            repeated.extend_from_slice(&payload[fields]);
            payload.truncate(payload.len() - 4);
            payload.extend(repeated);
            payload.extend([0, 1, 0, 1]);
        } else {
            let at = body.field_range("state").unwrap().start;
            payload[at] = 0;
        }
        assert!(bodies(&sequence(&payload).unwrap()).is_empty());
    }
}

#[test]
fn unterminated_node_sequences_do_not_count() {
    let mut payload = mesh_body_sequence(1, 1);
    payload.truncate(payload.len() - 4);
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&payload)
        .unwrap()
        .unwrap();
    assert!(!records.is_terminated());
    assert!(bodies(&records).is_empty());
}
