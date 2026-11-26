// SPDX-License-Identifier: Apache-2.0

use crate::scalar::PositiveReal;
use crate::subd::{
    SubdCage, SubdGripDirection, SubdGripWedge, SubdSecondaryGrip, SubdVertexGripLayout,
};
use cadmpeg_core::serde_erase::json;

fn gripped_cage() -> SubdCage {
    let mut cage = super::triangle_cage();
    let grip = |source_index| {
        Some(SubdSecondaryGrip {
            source_index,
            point: crate::features::geometry::FinitePoint3::ZERO,
            weight: PositiveReal::new(1.0).unwrap(),
        })
    };
    cage.vertices[0].secondary_grips = Some(SubdVertexGripLayout {
        direction: SubdGripDirection::North,
        wedges: vec![SubdGripWedge::Slot {
            edge: Some(0),
            sector_face: Some(0),
            spokes: vec![grip(42)],
            sectors: vec![grip(43)],
        }],
    });
    cage
}

#[test]
fn cage_validation_keeps_a_gripped_cage_and_names_a_missing_face_edge() {
    let cage = gripped_cage();
    let expected = cage.clone();
    assert_eq!(
        SubdCage::new(cage.vertices, cage.edges, cage.faces, cage.symmetries).unwrap(),
        expected
    );
    let mut cage = super::triangle_cage();
    cage.faces[0] = crate::subd::SubdFace::from_edge_uses(
        cage.faces[0]
            .edge_uses()
            .zip([None, Some(2), Some(u32::MAX)])
            .map(|(use_, edge)| crate::subd::SubdEdgeUse {
                edge: edge.unwrap_or(use_.edge),
                reversed: use_.reversed,
            })
            .collect(),
    )
    .unwrap();
    let wire = json::to_value(&cage).unwrap();
    let error = SubdCage::new(cage.vertices, cage.edges, cage.faces, cage.symmetries).unwrap_err();
    assert_eq!(
        error.to_string(),
        "faces[0].edges references a missing edge"
    );
    assert!(json::from_value::<SubdCage>(wire)
        .unwrap_err()
        .to_string()
        .contains("faces[0].edges references a missing edge"));
}
