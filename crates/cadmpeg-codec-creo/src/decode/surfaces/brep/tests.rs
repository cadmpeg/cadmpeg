// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::{split_neutral_component_shells, NeutralShellSpec};

#[test]
fn partitions_face_shells_and_retains_unattached_wire_curves() {
    let faces = [1, 2, 3];
    let face_adjacency = BTreeMap::from([
        (1, BTreeSet::from([2])),
        (2, BTreeSet::from([1])),
        (3, BTreeSet::new()),
    ]);
    let face_vertices = BTreeMap::from([
        (1, BTreeSet::from([10, 11])),
        (2, BTreeSet::from([11, 12])),
        (3, BTreeSet::from([30, 31])),
    ]);
    let edge_vertices = BTreeMap::from([(100, [11, 12]), (101, [40, 41])]);

    let shells = split_neutral_component_shells(
        &faces,
        &BTreeSet::from([100, 101]),
        &face_adjacency,
        &face_vertices,
        &edge_vertices,
    );

    assert_eq!(
        shells,
        vec![
            NeutralShellSpec {
                faces: vec![1, 2],
                wire_curves: BTreeSet::from([100]),
            },
            NeutralShellSpec {
                faces: vec![3],
                wire_curves: BTreeSet::new(),
            },
            NeutralShellSpec {
                faces: Vec::new(),
                wire_curves: BTreeSet::from([101]),
            },
        ]
    );
}
