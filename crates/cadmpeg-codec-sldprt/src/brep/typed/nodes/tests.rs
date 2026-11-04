// SPDX-License-Identifier: Apache-2.0

use super::facts;
use crate::brep::topology::nodes::sequence;
use crate::brep::typed::RegionKind;
use crate::test_support::parasolid::sequence::topology_sequence;
use cadmpeg_ir::topology::{BodyKind, Sense};
use std::collections::HashSet;

#[test]
fn ownership_nodes_close_one_body_hierarchy() {
    let (payload, body_offset) = topology_sequence();
    let records = sequence(&payload).unwrap();
    let facts = facts(&records, body_offset).unwrap();

    let [body] = facts.bodies.as_slice() else {
        panic!("one BODY")
    };
    assert_eq!(
        (body.attr, body.node_id, body.kind),
        (2, 10, BodyKind::Solid)
    );
    assert_eq!(body.topology_refs, [4, 1, 1, 1, 3, 1, 1]);
    let [region] = facts.regions.as_slice() else {
        panic!("one REGION")
    };
    assert_eq!(
        (region.refs, region.kind),
        ([1, 2, 1, 1, 4], RegionKind::Solid)
    );
    let [shell] = facts.shells.as_slice() else {
        panic!("one SHELL")
    };
    assert_eq!(shell.refs, [1, 2, 1, 5, 1, 1, 3, 1]);
    let [face] = facts.faces.as_slice() else {
        panic!("one FACE")
    };
    assert_eq!((face.refs, face.sense), ([1, 1, 6, 4, 20], Sense::Forward));

    let hierarchies = facts
        .hierarchies(&HashSet::from([5]))
        .expect("the ownership closes");
    assert_eq!(hierarchies.len(), 1);
    assert_eq!(hierarchies[0].body.attr, 2);
}

#[test]
fn a_body_with_an_invalid_resolution_is_omitted() {
    let (mut payload, body_offset) = topology_sequence();
    let records = sequence(&payload).unwrap();
    let body = records.nodes().find(|node| node.kind() == 12).unwrap();
    let at = body.field_range("res_size").unwrap().start;
    payload[at..at + 8].copy_from_slice(&0.0_f64.to_be_bytes());
    let records = sequence(&payload).unwrap();
    let facts = facts(&records, body_offset).unwrap();
    assert!(facts.bodies.is_empty());
    assert_eq!(facts.faces.len(), 1);
}
