// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::super::BrepComponentTopology;

fn topology_result(two_faces: bool) -> BrepComponentTopology {
    let first_loop = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::Zero,
        }],
    );
    let second_loop = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(6),
        vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::One,
        }],
    );
    let mut eligible_faces = BTreeMap::from([(5, vec![&first_loop])]);
    if two_faces {
        eligible_faces.insert(6, vec![&second_loop]);
    }
    let faces: &[u32] = if two_faces { &[5, 6] } else { &[5] };
    BrepComponentTopology::from_component(
        &BTreeSet::from([10, 11]),
        &BTreeSet::from([10]),
        faces,
        &eligible_faces,
        &BTreeMap::from([(10, [1, 2])]),
    )
}

#[test]
fn brep_component_topology_preserves_service_incidence() {
    let topology = topology_result(true);
    assert_eq!(topology.component_face_curves, BTreeSet::from([10]));
    assert_eq!(topology.wire_curves, BTreeSet::from([11]));
    assert_eq!(topology.face_adjacency[&5], BTreeSet::from([6]));
    assert_eq!(topology.face_adjacency[&6], BTreeSet::from([5]));
    assert_eq!(topology.face_vertices[&5], BTreeSet::from([1, 2]));
    assert_eq!(topology.face_vertices[&6], BTreeSet::from([1, 2]));
}
