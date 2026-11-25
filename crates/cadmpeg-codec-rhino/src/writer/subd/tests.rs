// SPDX-License-Identifier: Apache-2.0
//! `ON_SubD` payloads read back through the decoder to the cage they state.

use cadmpeg_ir::ids::SubdId;
use cadmpeg_ir::subd::{SubdCage, SubdEdge};

use super::{control_mesh, payload, placed, Unheld, SUBD_CLASS};
use crate::chunks::ArchiveVersion;
use crate::settings::MillimeterScale;
use crate::test_support::{start_sharpness_cage, two_quad_cage};
use crate::RhinoArchiveVersion;

fn read_back(cage: &SubdCage, version: RhinoArchiveVersion) -> (SubdCage, Vec<Unheld>) {
    let written = payload(cage, version).expect("the payload is written");
    assert_eq!(written.direct, [1]);
    let decoded = crate::subd::decode(
        &written.body,
        0..written.body.len(),
        ArchiveVersion::from_word(version.value()),
        MillimeterScale::IDENTITY,
        SubdId::mint("test:rhino:subd#written").expect("identity grammar"),
    )
    .expect("the payload decodes")
    .expect("the payload holds a SubDimple");
    assert_eq!(decoded.warnings.messages().count(), 0);
    assert!(!decoded.neutral_metadata);
    (decoded.surface.cage, written.unheld)
}

#[test]
fn the_class_id_is_the_on_subd_class() {
    assert_eq!(SUBD_CLASS, crate::subd::ON_SUBD.to_wire());
}

#[test]
fn an_archive_80_payload_reads_back_as_its_cage() {
    let cage = two_quad_cage();
    assert_eq!(
        read_back(&cage, RhinoArchiveVersion::V8),
        (cage, Vec::new())
    );
}

#[test]
fn an_earlier_payload_holds_the_start_sharpness_at_both_ends() {
    let cage = two_quad_cage();
    let expected = start_sharpness_cage(&cage);
    for version in [RhinoArchiveVersion::V6, RhinoArchiveVersion::V7] {
        assert_eq!(
            read_back(&cage, version),
            (expected.clone(), vec![Unheld::EndSharpness]),
            "{version:?}"
        );
    }
}

#[test]
fn a_cage_of_no_vertex_reads_back_empty() {
    let cage = SubdCage::default();
    assert_eq!(
        read_back(&cage, RhinoArchiveVersion::V8),
        (cage, Vec::new())
    );
}

#[test]
fn a_placed_cage_moves_its_vertices() {
    let cage = two_quad_cage();
    let shift = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 10.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, -2.0],
    ])
    .expect("an affine placement");
    let moved = placed(&cage, Some(shift)).expect("the placement is finite");
    for (before, after) in cage.vertices().iter().zip(moved.vertices()) {
        let [before, after] = [before.point().get(), after.point().get()];
        assert_eq!(
            [after.x, after.y, after.z],
            [before.x + 10.0, before.y, before.z - 2.0]
        );
    }
    assert_eq!(moved.edges(), cage.edges());
}

#[test]
fn a_control_mesh_holds_the_cage_faces_as_quads() {
    let mesh = control_mesh(&two_quad_cage()).expect("the control net is a mesh");
    assert_eq!(mesh.faces, [[0, 1, 4, 3], [1, 2, 5, 4]]);
    assert_eq!(mesh.vertices.len(), 6);
    assert!(!mesh.fanned);
}

#[test]
fn a_control_mesh_fans_a_face_of_more_than_four_sides() {
    use cadmpeg_ir::subd::{SubdEdgeTag, SubdEdgeUse, SubdFace, SubdVertex, SubdVertexTag};
    let corners = [[0.0, 0.0], [2.0, 0.0], [3.0, 1.0], [1.0, 2.0], [-1.0, 1.0]];
    let vertices = corners
        .iter()
        .map(|&[x, y]| {
            SubdVertex::new(
                cadmpeg_ir::math::Point3::new(x, y, 0.0),
                SubdVertexTag::Corner,
                None,
            )
            .expect("a finite vertex")
        })
        .collect();
    let edges = (0..5_u32)
        .map(|index| {
            SubdEdge::new(
                [index, (index + 1) % 5],
                [0.0, 0.0],
                SubdEdgeTag::Crease,
                None,
                [0.0, 0.0],
            )
            .expect("a valid edge")
        })
        .collect();
    let face = SubdFace::from_edge_uses(
        (0..5)
            .map(|edge| SubdEdgeUse {
                edge,
                reversed: false,
            })
            .collect(),
    )
    .expect("a valid face");
    let cage = SubdCage::new(vertices, edges, vec![face], Vec::new()).expect("a valid cage");
    let mesh = control_mesh(&cage).expect("the control net is a mesh");
    assert_eq!(mesh.faces, [[0, 1, 2, 2], [0, 2, 3, 3], [0, 3, 4, 4]]);
    assert!(mesh.fanned);
}

#[test]
fn a_cage_of_no_face_has_no_control_mesh() {
    assert!(control_mesh(&SubdCage::default()).is_err());
}
