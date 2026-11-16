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

#[test]
fn explicit_null_faces_are_removed_before_half_edge_indexes_are_built() {
    use crate::surface::owners::{SurfaceOwner, SurfaceRowOwner};
    use crate::topology::Side;

    let half_edges = |face_id, curve_ids: &[u32], first_side| {
        crate::test_support::closed_loop(
            std::num::NonZeroU32::new(face_id),
            curve_ids
                .iter()
                .enumerate()
                .map(|(index, curve_id)| crate::topology::HalfEdgeId {
                    curve_id: *curve_id,
                    side: if index == 0 { first_side } else { Side::Zero },
                })
                .collect(),
        )
    };
    let body_loop = half_edges(5, &[10, 11, 12, 13], Side::Zero);
    let unowned_loop = half_edges(6, &[10, 14, 15, 16], Side::One);
    let mut eligible_faces = BTreeMap::from([(5, vec![&body_loop]), (6, vec![&unowned_loop])]);
    let mut face_regions = BTreeMap::new();
    let mut scan = crate::test_support::empty_container_scan();
    for (offset, (surface_id, owner)) in [(5, SurfaceOwner::Body(368)), (6, SurfaceOwner::Unowned)]
        .into_iter()
        .enumerate()
    {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id: surface_id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset,
        });
        scan.surfaces.row_owners.push(SurfaceRowOwner {
            surface_id,
            owner,
            offset,
        });
    }
    let mut losses = Vec::new();
    let (unowned_faces, unowned_loop_curves, body_loop_curves) =
        super::super::exclude_explicit_unowned_faces(
            &scan,
            &mut eligible_faces,
            &mut face_regions,
            &mut losses,
        );
    assert_eq!(unowned_faces, BTreeSet::from([6]));
    assert!(face_regions.is_empty());
    assert_eq!(losses.len(), 1);
    let rows = [
        (10, 5, 6),
        (11, 5, 0),
        (12, 5, 0),
        (13, 5, 0),
        (14, 6, 0),
        (15, 6, 0),
        (16, 6, 0),
    ]
    .map(
        |(id, first_face, second_face)| crate::curve::CurveTopologyRow {
            id,
            type_byte: 0,
            feature_id: 1,
            directions: [0, 0],
            faces: [
                std::num::NonZeroU32::new(first_face),
                std::num::NonZeroU32::new(second_face),
            ],
            next_edges: [0, 0],
            offset: usize::try_from(id).expect("test offset fits usize"),
        },
    );
    let indexes = BrepEligibleFaceIndexes::from_faces(&eligible_faces, &rows);
    assert_eq!(indexes.eligible_face_ids, BTreeSet::from([5]));
    assert_eq!(indexes.face_curves, BTreeSet::from([10, 11, 12, 13]));
    assert!(indexes.closed_single_edge_curves.is_empty());
    assert!(indexes
        .emitted_half_edges
        .contains(&crate::topology::HalfEdgeId {
            curve_id: 10,
            side: Side::Zero,
        }));
    assert!(!indexes
        .emitted_half_edges
        .contains(&crate::topology::HalfEdgeId {
            curve_id: 10,
            side: Side::One,
        }));
    assert_eq!(unowned_loop_curves, BTreeSet::from([10, 14, 15, 16]));
    assert_eq!(body_loop_curves, BTreeSet::from([10, 11, 12, 13]));
}
