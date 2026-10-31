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

#[test]
fn a_b_rep_with_no_trim_curves_reads_with_derived_pcurves() {
    let mut packed = Packed::default();
    packed.push(&cube(), false).expect("encode");
    assert!(packed.trim_curves.is_none());
    let decoded = decode(&packed).expect("decode");
    let brep = &decoded.breps[0];
    assert!(brep
        .oriented_edges
        .iter()
        .all(|oriented| oriented.pcurve.is_some()));
    for (face, record) in brep.faces.iter().enumerate() {
        let surface = &brep.nurbs_surfaces[face];
        for oriented in record
            .loops
            .iter()
            .flat_map(|loop_| &brep.loops[*loop_].oriented_edges)
        {
            let record = &brep.oriented_edges[*oriented];
            let (_, pcurve) = record.pcurve.expect("pcurve");
            let curve = &brep.nurbs_pcurves[pcurve];
            let edge = &brep.edges[record.edge];
            let end = if record.orientation {
                edge.vertices[1]
            } else {
                edge.vertices[0]
            };
            let last = crate::brep::convert::evaluate(curve, curve.knots[curve.points.len()])
                .expect("point");
            let point = crate::brep::convert::surface_point(surface, last).expect("point");
            for (value, expected) in point.iter().zip(brep.vertices[end]) {
                assert!((value - expected).abs() < 1e-9, "{face}");
            }
        }
    }
}

#[test]
fn identities_pack_and_decode_by_entity() {
    let mut brep = cube();
    brep.identities = crate::brep::Identities {
        body: Some("test:model:body#cube".to_owned()),
        faces: (0..brep.faces.len())
            .map(|index| Some(format!("test:model:face#{index}")))
            .collect(),
        edges: (0..brep.edges.len())
            .map(|index| (index % 2 == 0).then(|| format!("test:model:edge#{index}")))
            .collect(),
        vertices: Vec::new(),
        body_name: Some("Cube".to_owned()),
        face_names: (0..brep.faces.len())
            .map(|index| (index == 1).then(|| "Top".to_owned()))
            .collect(),
    };
    let mut packed = Packed::default();
    packed.push(&brep, true).expect("encode");
    assert!(packed.identities.any());
    assert_eq!(
        packed.identities.vertices,
        vec![String::new(); brep.vertices.len()]
    );
    let decoded = decode(&packed).expect("decode");
    let read = &decoded.breps[0].identities;
    assert_eq!(read.body.as_deref(), Some("test:model:body#cube"));
    assert_eq!(read.faces, brep.identities.faces);
    let mut edges: Vec<String> = read.edges.iter().flatten().cloned().collect();
    edges.sort();
    let mut expected: Vec<String> = brep.identities.edges.iter().flatten().cloned().collect();
    expected.sort();
    assert_eq!(edges, expected);
    assert!(read.vertices.iter().all(Option::is_none));
    assert_eq!(read.body_name.as_deref(), Some("Cube"));
    assert_eq!(read.face_names, brep.identities.face_names);
}

#[test]
fn tolerances_pack_and_decode_by_entity() {
    let mut brep = cube();
    brep.tolerances = crate::brep::Tolerances {
        faces: (0..brep.faces.len())
            .map(|index| (index == 2).then_some(1e-4))
            .collect(),
        edges: Vec::new(),
        vertices: (0..brep.vertices.len())
            .map(|index| (index == 0).then_some(2e-4))
            .collect(),
    };
    let mut packed = Packed::default();
    packed.push(&brep, true).expect("encode");
    assert!(packed.tolerances.any());
    assert!(packed.tolerances.edges.iter().all(|value| value.is_nan()));
    let decoded = decode(&packed).expect("decode");
    let read = &decoded.breps[0].tolerances;
    assert_eq!(read.faces, brep.tolerances.faces);
    assert!(read.edges.iter().all(Option::is_none));
    assert_eq!(read.vertices.iter().flatten().collect::<Vec<_>>(), [&2e-4]);
}
