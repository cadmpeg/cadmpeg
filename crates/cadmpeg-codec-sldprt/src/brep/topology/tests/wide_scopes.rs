// SPDX-License-Identifier: Apache-2.0

use crate::brep::topology::{topology_header_prefix_end, topology_scopes};
use crate::test_support::parasolid::{framed_topology_fin, wide};

#[test]
fn wide_edge_scope_keeps_a_vertex_tag_in_its_owner_field() {
    let edge = wide::edge(0x1_0000, 18);
    let next = framed_topology_fin(31, [1, 20, 32, 30, 50, 1, 40, 1, 1], false, false, false);
    let body = [edge.as_slice(), next.as_slice()].concat();
    assert_eq!(topology_header_prefix_end(&body, 0, 0x10), Some(edge.len()));
    let scopes = topology_scopes(&body, false);
    assert!(scopes
        .excluded_starts
        .iter()
        .any(|range| range.contains(&(edge.len() - 2))));
    assert!(!scopes
        .excluded_starts
        .iter()
        .any(|range| range.contains(&edge.len())));
}

#[test]
fn wide_integer_payload_keeps_its_topology_tags_inside_the_array() {
    let mut fin = framed_topology_fin(31, [1, 20, 32, 30, 50, 1, 40, 1, 1], false, false, false);
    while !fin.len().is_multiple_of(4) {
        fin.push(0);
    }
    let values = (0..fin.len())
        .step_by(4)
        .map(|at| cadmpeg_core::decode::View::u32_be_at(&fin, at).unwrap())
        .collect::<Vec<_>>();
    let body = wide::integer_values(0x1_0000, &values);
    assert_eq!(topology_scopes(&body, false).payloads, vec![0..body.len()]);
}

#[test]
fn wide_attribute_scope_covers_the_definition_and_slot_pointers() {
    let body = wide::attribute(0x1_0000, 0x1_0001, &[17, 18, 0x1_0002, 1, 1, 0x1_0003]);
    assert_eq!(topology_scopes(&body, false).payloads, vec![0..body.len()]);
}

#[test]
fn selected_wide_fin_keeps_its_own_identity_under_prefix_ambiguity() {
    let mut references = [1; 9];
    references[8] = (0x012b << 15) | 0x2b;
    let body = wide::fin(0xfffd, &references);
    let tables = crate::brep::topology::scan_plain(&body);
    let fin = tables.coedges().get(&0xfffd).expect("wide FIN identity");
    assert_eq!(fin.attr, 0xfffd);
    assert_eq!(fin.refs, references);
    assert_eq!(tables.coedges().len(), 1);
    assert!(!tables.coedge_is_ambiguous(0xfffd));
}

#[test]
fn wide_fin_scope_covers_a_vertex_tag_in_a_reference() {
    let mut references = [1; 9];
    references[3] = 18 << 15;
    let body = wide::fin(0x1_0000, &references);
    let nested = body.windows(2).position(|word| word == [0, 18]).unwrap();
    assert_eq!(topology_header_prefix_end(&body, 0, 0x11), Some(body.len()));
    assert!(topology_scopes(&body, false)
        .excluded_starts
        .iter()
        .any(|range| range.contains(&nested)));
}

#[test]
fn string_node_scopes_follow_the_native_identity_width() {
    for tag in [0x4f, 0x54] {
        for key in [0x7fff, 0x8000, 0xffff, 0x1_0000, 18 << 15] {
            let node = wide::string_node(tag, key, "label");
            let fin = framed_topology_fin(31, [1; 9], false, false, false);
            let body = [node.as_slice(), fin.as_slice()].concat();
            assert_eq!(
                topology_scopes(&body, false).excluded_starts[0],
                1..node.len()
            );
            assert!(crate::brep::topology::scan_plain(&body)
                .coedges()
                .contains_key(&31));
        }
    }
}
