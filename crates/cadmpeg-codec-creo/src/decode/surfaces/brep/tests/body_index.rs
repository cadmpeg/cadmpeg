// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::super::{used_brep_vertices, BrepBodyIndexes, NeutralShellSpec};

fn index_result() -> BrepBodyIndexes {
    let mut scan = crate::test_support::empty_container_scan();
    scan.framing.layout = crate::container::Layout::Nd;
    let component = crate::topology::FaceComponent::new_for_test(vec![5], vec![10])
        .expect("valid component fixture");
    let admitted_components = [&component];
    let admitted_edge_curves = BTreeSet::from([10]);
    let eligible_faces = BTreeMap::from([(5, Vec::new())]);
    let eligible_face_ids = BTreeSet::from([5]);
    let curve_faces = BTreeMap::new();
    BrepBodyIndexes::from_components(
        &scan,
        &admitted_components,
        &admitted_edge_curves,
        &eligible_faces,
        &eligible_face_ids,
        &curve_faces,
    )
}

#[test]
fn brep_body_indexes_preserve_service_values() {
    let indexes = index_result();
    assert_eq!(indexes.neutral_edge_curves, BTreeSet::from([10]));
    assert_eq!(
        indexes.body_components,
        vec![NeutralShellSpec {
            faces: vec![5],
            wire_curves: BTreeSet::from([10]),
        }]
    );
    let vertices = {
        used_brep_vertices(
            &indexes.neutral_edge_curves,
            &BTreeMap::from([(10, [1, 2])]),
        )
    };
    assert_eq!(vertices, BTreeSet::from([1, 2]));
}
