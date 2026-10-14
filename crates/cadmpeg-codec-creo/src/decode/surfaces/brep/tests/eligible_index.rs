// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::super::BrepEligibleFaceIndexes;

fn source_loop() -> crate::topology::Loop {
    crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::Zero,
        }],
    )
}

fn source_row() -> crate::curve::CurveTopologyRow {
    crate::curve::CurveTopologyRow {
        id: 10,
        type_byte: 0,
        feature_id: 1,
        directions: [0, 0],
        faces: [std::num::NonZeroU32::new(5), None],
        next_edges: [0, 0],
        offset: 12,
    }
}

#[test]
fn brep_eligible_face_indexes_record_the_single_edge_face() {
    let lp = source_loop();
    let faces = BTreeMap::from([(5, vec![&lp])]);
    let rows = [source_row()];
    let indexes = BrepEligibleFaceIndexes::from_faces(&faces, &rows);
    assert_eq!(
        indexes.emitted_half_edges,
        BTreeSet::from([lp.half_edges()[0]])
    );
    assert_eq!(indexes.face_curves, BTreeSet::from([10]));
    assert_eq!(indexes.closed_single_edge_curves, BTreeSet::from([10]));
    assert_eq!(indexes.row_offsets[&10], 12);
    assert_eq!(indexes.curve_faces[&10], [5, 0]);
    assert_eq!(indexes.eligible_face_ids, BTreeSet::from([5]));
}
