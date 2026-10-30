// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{decode, Packed, NO_VERTEX};
use crate::brep::convert::nurbs_only;

fn cube() -> crate::brep::Brep {
    nurbs_only(&crate::brep::primitive::cube(2.0))
        .expect("convert")
        .brep
}

#[test]
fn a_solid_has_a_void_region_and_a_solid_region() {
    let brep = cube();
    let mut packed = Packed::default();
    packed.push(&brep, true).expect("encode");
    assert_eq!(packed.region_count, [2]);
    assert_eq!(packed.region_solid, [false, true]);
    assert_eq!(packed.region_shell_count, [1, 1]);
    assert_eq!(packed.shell_faceuse_count, [6, 6]);
    assert_eq!(packed.faceuse_face.len(), 12);
    for face in 0..6 {
        let uses: Vec<bool> = packed
            .faceuse_face
            .iter()
            .zip(&packed.faceuse_same)
            .filter(|(index, _)| **index == face)
            .map(|(_, same)| *same)
            .collect();
        assert_eq!(uses.len(), 2);
        assert_ne!(uses[0], uses[1]);
    }
    assert_eq!(packed.loop_edgeuse_count, [4; 6]);
    assert!(packed.loop_vertex.iter().all(|vertex| *vertex == NO_VERTEX));
    assert_eq!(packed.edgeuse_edge.len(), 24);
    assert_eq!(packed.edge_range.len(), 2 * packed.edge_vertices.len());
    assert_eq!(
        packed
            .trim_curves
            .as_ref()
            .map(|trims| trims.vertex_count.len()),
        Some(24)
    );
}

#[test]
fn each_manifold_edge_has_a_two_member_radial_ring() {
    let mut packed = Packed::default();
    packed.push(&cube(), true).expect("encode");
    let mut uses: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (edgeuse, edge) in packed.edgeuse_edge.iter().enumerate() {
        uses.entry(*edge).or_default().push(edgeuse);
    }
    for pair in uses.values() {
        let [a, b] = pair.as_slice() else {
            panic!("{pair:?}");
        };
        assert_eq!(
            usize::try_from(packed.edgeuse_next_radial[*a]).expect("index"),
            *b
        );
        assert_eq!(
            usize::try_from(packed.edgeuse_next_radial[*b]).expect("index"),
            *a
        );
        assert_ne!(packed.edgeuse_same[*a], packed.edgeuse_same[*b]);
        assert_ne!(packed.edgeuse_top_entry[*a], packed.edgeuse_top_entry[*b]);
    }
}

#[test]
fn encoded_records_decode_to_the_same_topology() {
    let brep = cube();
    let mut packed = Packed::default();
    packed.push(&brep, true).expect("encode");
    let decoded = decode(&packed).expect("decode");
    let [read] = decoded.breps.as_slice() else {
        panic!("one B-rep");
    };
    assert_eq!(read.faces.len(), brep.faces.len());
    assert_eq!(read.edges.len(), brep.edges.len());
    assert_eq!(read.vertices.len(), brep.vertices.len());
    assert_eq!(read.solids.len(), 1);
    assert_eq!(read.shells.len(), 1);
    assert!(read.shells[0].closed);
    for (written, read) in brep.faces.iter().zip(&read.faces) {
        assert_eq!(written.same_sense, read.same_sense);
        assert_eq!(written.loops.len(), read.loops.len());
    }
    assert_eq!(read.nurbs_pcurves.len(), 24);
    assert_eq!(decoded.untransferred, 0);
}

#[test]
fn two_b_reps_pack_after_one_another() {
    let brep = cube();
    let mut packed = Packed::default();
    packed.push(&brep, true).expect("first");
    packed.push(&brep, true).expect("second");
    assert_eq!(packed.region_count, [2, 2]);
    assert_eq!(packed.extent.len(), 4);
    let decoded = decode(&packed).expect("decode");
    assert_eq!(decoded.breps.len(), 2);
    assert_eq!(decoded.faces[1], (6..12).collect::<Vec<_>>());
    assert!(packed.push(&brep, false).is_err());
}

#[test]
fn a_face_with_one_faceuse_is_refused() {
    let mut packed = Packed::default();
    packed.push(&cube(), true).expect("encode");
    packed.faceuse_face.pop();
    packed.faceuse_same.pop();
    packed.shell_faceuse_count[1] -= 1;
    assert!(decode(&packed).is_err());
}

#[test]
fn a_face_between_two_solid_regions_is_refused() {
    let mut packed = Packed::default();
    packed.push(&cube(), true).expect("encode");
    packed.region_solid[0] = true;
    let error = decode(&packed).expect_err("refused");
    assert!(error.contains("two solid regions"), "{error}");
}
