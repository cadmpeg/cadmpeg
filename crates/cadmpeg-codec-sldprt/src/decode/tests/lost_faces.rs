// SPDX-License-Identifier: Apache-2.0
//! Decode tests of the body, region, and shell records whose faces are lost
//! (CADIR §3.3, §5.6).
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::topology::{BodyKind, RegionUnstated, Unstated};
use cadmpeg_ir::CadIr;

use crate::test_support::container::{make_block, outer_header};
use crate::test_support::parasolid::sequence::{
    face_fields, loopless_nodes, sequence_of, F, NULL_REAL,
};
use crate::SldprtCodec;

type Nodes = Vec<(u16, u16, Vec<F>)>;

fn decode(nodes: &Nodes) -> (CadIr, Vec<String>) {
    let (payload, _) = sequence_of(nodes);
    let mut file = outer_header();
    file.extend_from_slice(&make_block(0x20, "Contents/Config-0-Partition", &payload));
    let result = SldprtCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model")
        .is_ok());
    let messages = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.message.clone())
        .collect();
    (result.ir().clone(), messages)
}

fn ids<'a>(ids: impl IntoIterator<Item = &'a cadmpeg_ir::ids::ShellId>) -> Vec<&'a str> {
    ids.into_iter()
        .map(cadmpeg_ir::ids::ShellId::as_str)
        .collect()
}

/// A second shell 6 of region 3 that holds FACE 7, which names no loop, on
/// a surface 21 of node `kind`.
fn second_shell(nodes: &mut Nodes, kind: u16) {
    use F::{C, I, P, R, V};
    nodes[2].2[3] = P(6);
    #[rustfmt::skip]
    nodes.push((13, 6, vec![I(11), P(1), P(2), P(1), P(7), P(1), P(1), P(3), P(1)]));
    let mut face = face_fields();
    face[0] = I(12);
    face[5] = P(1);
    face[6] = P(6);
    face[7] = P(21);
    nodes.push((14, 7, face));
    let mut surface = vec![I(13), P(1), P(1), P(1), P(1), P(1), C(b'+')];
    match kind {
        50 => surface.extend([V([0.0; 3]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0])]),
        53 => surface.extend([
            V([0.0, 0.0, 0.05]),
            R(0.007),
            V([0.0, 0.0, 1.0]),
            V([1.0, 0.0, 0.0]),
        ]),
        _ => unreachable!(),
    }
    nodes.push((kind, 21, surface));
}

/// A LOOP record that names FACE `face`, which names no loop: the face does
/// not cover its whole closed surface, so it is given up.
fn loop_naming(nodes: &mut Nodes, attr: u16, face: u16) {
    use F::{I, P};
    nodes.push((15, attr, vec![I(30), P(1), P(1), P(face), P(1)]));
}

/// The nodes of a sheet SHELL 4 that lists a plane FACE 5 and a plane FACE
/// 12, each bounded by an ellipse ring at its own centre, so they share no
/// edge, and between them in the face chain a loopless sphere FACE 9 that a
/// LOOP names, so it is bridged and given up.
fn split_shell_nodes() -> Nodes {
    use F::{C, I, P, R, V};
    let mut nodes = loopless_nodes(50);
    nodes[0].2[14] = F::B(3);
    nodes[3].2[3] = P(9);
    nodes[3].2[5] = P(6);
    let mut sphere = face_fields();
    sphere[0] = I(9);
    sphere[3] = P(12);
    sphere[4] = P(5);
    sphere[5] = P(1);
    sphere[7] = P(24);
    let mut plane = face_fields();
    plane[0] = I(10);
    plane[4] = P(9);
    plane[5] = P(13);
    plane[7] = P(23);
    #[rustfmt::skip]
    nodes.extend([
        (15, 6, vec![I(5), P(1), P(7), P(5), P(1)]),
        (17, 7, vec![P(1), P(6), P(7), P(7), P(1), P(11), P(8), P(1), P(1), C(b'+')]),
        (17, 11, vec![P(1), P(1), P(1), P(1), P(1), P(7), P(8), P(1), P(1), C(b'-')]),
        (16, 8, vec![I(6), P(1), R(NULL_REAL), P(7), P(1), P(1), P(21), P(1), P(1), P(2)]),
        (32, 21, vec![I(7), P(1), P(1), P(1), P(1), P(1), C(b'+'), V([0.0; 3]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0]), R(0.007), R(0.002)]),
        (14, 9, sphere),
        (53, 24, vec![I(14), P(1), P(1), P(1), P(1), P(1), C(b'+'), V([0.0, 0.1, 0.0]), R(0.007), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0])]),
        (15, 30, vec![I(30), P(1), P(1), P(9), P(1)]),
        (14, 12, plane),
        (50, 23, vec![I(15), P(1), P(1), P(1), P(1), P(1), C(b'+'), V([0.05, 0.0, 0.0]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0])]),
        (15, 13, vec![I(16), P(1), P(14), P(12), P(1)]),
        (17, 14, vec![P(1), P(13), P(14), P(14), P(1), P(15), P(16), P(1), P(1), C(b'+')]),
        (17, 15, vec![P(1), P(1), P(1), P(1), P(1), P(14), P(16), P(1), P(1), C(b'-')]),
        (16, 16, vec![I(17), P(1), R(NULL_REAL), P(14), P(1), P(1), P(22), P(1), P(1), P(2)]),
        (32, 22, vec![I(18), P(1), P(1), P(1), P(1), P(1), C(b'+'), V([0.05, 0.0, 0.0]), V([0.0, 0.0, 1.0]), V([1.0, 0.0, 0.0]), R(0.007), R(0.002)]),
    ]);
    nodes
}

/// One stored SHELL that a lost face splits into pieces states the pieces
/// without saying which bounds the outside: no stored field says so, and
/// none of them is a cavity (CADIR §5.6).
#[test]
fn a_shell_split_by_a_lost_face_leaves_its_pieces_unclassified() {
    let (ir, messages) = decode(&split_shell_nodes());
    let geometry = &ir.geometry;
    let faces: Vec<_> = geometry.faces.iter().map(|face| face.id.as_str()).collect();
    assert_eq!(
        faces,
        ["sldprt:brep:face#12", "sldprt:brep:face#5"],
        "{messages:#?}"
    );
    let [region] = geometry.regions.as_slice() else {
        panic!("one region: {messages:#?}");
    };
    let Some(RegionUnstated::Unclassified { shells }) = &region.unstated else {
        panic!("unclassified shells: {region:#?}");
    };
    assert_eq!(
        ids(shells),
        ["sldprt:brep:shell#4", "sldprt:brep:shell#4.component-1"]
    );
    assert!(geometry
        .shells
        .iter()
        .all(|shell| shell.unstated == Some(Unstated::Members)));
    assert!(
        messages.iter().any(|message| message.contains("face#9")),
        "{messages:#?}"
    );
}

/// A derived hierarchy whose faces form two disjoint pieces states both
/// without saying which bounds the outside: no record states the region or
/// its shells (CADIR §5.6).
#[test]
fn a_derived_region_of_disjoint_pieces_leaves_them_unclassified() {
    let mut nodes = two_sphere_shells(false);
    nodes.retain(|(kind, _, _)| *kind != 12);
    let (ir, messages) = decode(&nodes);
    let geometry = &ir.geometry;
    assert_eq!(geometry.faces.len(), 2, "{messages:#?}");
    assert_eq!(geometry.bodies[0].kind, BodyKind::Solid);
    let [region] = geometry.regions.as_slice() else {
        panic!("one region: {messages:#?}");
    };
    let Some(RegionUnstated::Unclassified { shells }) = &region.unstated else {
        panic!("unclassified shells: {region:#?}");
    };
    assert_eq!(
        ids(shells),
        ["sldprt:brep:shell#0", "sldprt:brep:shell#0.component-1"]
    );
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("No body record names a region;")),
        "{messages:#?}"
    );
}

fn two_sphere_shells(nested: bool) -> Nodes {
    use F::{C, I, P, R, V};
    let mut nodes = loopless_nodes(53);
    nodes[0].2[20] = P(30);
    nodes[1].2[3] = P(if nested { 32 } else { 1 });
    nodes[1].2[4] = P(30);
    nodes[2].2[3] = P(7);
    nodes[3].2[8] = C(if nested { b'-' } else { b'+' });
    nodes[3].2[13] = P(33);
    let mut outer_face = face_fields();
    outer_face[0] = I(8);
    outer_face[5] = P(1);
    outer_face[6] = P(7);
    outer_face[7] = P(21);
    outer_face[13] = P(31);
    let mut outer_surface = nodes[4].2.clone();
    outer_surface[0] = I(9);
    outer_surface[7] = V(if nested { [0.0; 3] } else { [0.05, 0.0, 0.0] });
    outer_surface[8] = R(0.02);
    #[rustfmt::skip]
    nodes.extend([
        (13, 7, vec![I(10), P(1), P(2), P(1), P(12), P(1), P(1), P(3), P(1)]),
        (14, 12, outer_face),
        (53, 21, outer_surface),
        (19, 30, vec![I(11), P(1), P(2), P(3), P(1), P(31), C(b'V')]),
        (13, 31, vec![I(12), P(1), P(1), P(if nested { 1 } else { 33 }), P(1), P(1), P(1), P(30), P(12)]),
        (13, 33, vec![I(13), P(1), P(1), P(1), P(1), P(1), P(1), P(if nested { 32 } else { 30 }), P(5)]),
    ]);
    if nested {
        nodes.push((19, 32, vec![I(14), P(1), P(2), P(1), P(3), P(33), C(b'V')]));
    }
    nodes
}

#[test]
fn native_region_uses_the_boundary_toward_the_infinite_void_as_exterior() {
    let (ir, messages) = decode(&two_sphere_shells(true));
    assert_eq!(ir.geometry.faces.len(), 2, "{messages:#?}");
    let [region] = ir.geometry.regions.as_slice() else {
        panic!("one stated solid region: {messages:#?}");
    };
    assert_eq!(region.unstated, None);
    assert_eq!(
        ids(region.stated_shells()),
        ["sldprt:brep:shell#7", "sldprt:brep:shell#4"]
    );
}

#[test]
fn native_roles_survive_independent_cavity_evidence_failures() {
    for lose_carrier in [false, true] {
        let mut nodes = two_sphere_shells(true);
        if lose_carrier {
            nodes.retain(|(kind, attr, _)| (*kind, *attr) != (53, 20));
        } else {
            nodes[3].2[13] = F::P(1);
        }
        let (ir, messages) = decode(&nodes);
        assert_eq!(
            ir.geometry.faces.len(),
            if lose_carrier { 1 } else { 2 },
            "{messages:#?}"
        );
        let [region] = ir.geometry.regions.as_slice() else {
            panic!("one solid region: {messages:#?}");
        };
        assert_eq!(region.unstated, None, "{messages:#?}");
        assert_eq!(
            ids(region.stated_shells()),
            ["sldprt:brep:shell#7", "sldprt:brep:shell#4"]
        );
        if lose_carrier {
            let cavity = ir
                .geometry
                .shells
                .iter()
                .find(|shell| shell.id.as_str() == "sldprt:brep:shell#4")
                .unwrap();
            assert_eq!(cavity.unstated, Some(Unstated::Members));
            assert!(messages.iter().any(|message| message.contains("face#5")));
        }
    }
}

#[test]
fn an_unread_front_chain_leaves_its_shell_role_unresolved() {
    let mut nodes = two_sphere_shells(true);
    nodes[3].2[13] = F::P(1);
    nodes
        .iter_mut()
        .find(|(kind, attr, _)| (*kind, *attr) == (13, 33))
        .unwrap()
        .2[8] = F::P(1);
    let (ir, messages) = decode(&nodes);
    let [region] = ir.geometry.regions.as_slice() else {
        panic!("one solid region: {messages:#?}");
    };
    assert!(
        matches!(&region.unstated, Some(RegionUnstated::Unclassified { shells })
        if ids(shells) == ["sldprt:brep:shell#4", "sldprt:brep:shell#7"])
    );
    assert!(
        messages.iter().any(
            |message| message.contains("Stored face chains prove sldprt:brep:shell#7 exterior")
        ),
        "{messages:#?}"
    );
}

#[test]
fn a_cut_shell_chain_marks_its_missing_member_beside_proved_roles() {
    let mut nodes = two_sphere_shells(true);
    nodes
        .iter_mut()
        .find(|(kind, attr, _)| (*kind, *attr) == (13, 7))
        .unwrap()
        .2[3] = F::P(99);
    let (ir, messages) = decode(&nodes);
    let [region] = ir.geometry.regions.as_slice() else {
        panic!("one solid region: {messages:#?}");
    };
    assert_eq!(region.unstated, Some(RegionUnstated::Voids));
    assert_eq!(
        ids(region.stated_shells()),
        ["sldprt:brep:shell#7", "sldprt:brep:shell#4"]
    );
}

#[test]
fn a_manifold_general_body_keeps_the_roles_of_its_solid_region() {
    let mut nodes = two_sphere_shells(true);
    nodes[0].2[14] = F::B(6);
    nodes[0].2[16] = F::P(1);
    for (kind, _, fields) in &mut nodes {
        if *kind == 13 {
            fields[2] = F::P(1);
        }
    }
    nodes.retain(|(kind, attr, _)| (*kind, *attr) != (53, 20));
    let (ir, messages) = decode(&nodes);
    assert_eq!(ir.geometry.faces.len(), 1, "{messages:#?}");
    assert_eq!(
        ir.geometry.bodies[0].kind,
        BodyKind::General,
        "{messages:#?}"
    );
    let [region] = ir.geometry.regions.as_slice() else {
        panic!("one solid region: {messages:#?}");
    };
    assert_eq!(region.unstated, None);
    assert_eq!(
        ids(region.stated_shells()),
        ["sldprt:brep:shell#7", "sldprt:brep:shell#4"]
    );
}

#[test]
fn a_region_with_unclassified_shells_follows_the_stored_chain() {
    let mut nodes = two_sphere_shells(false);
    nodes[1].2[5] = F::P(7);
    nodes[2].2[3] = F::P(1);
    nodes
        .iter_mut()
        .find(|(kind, attr, _)| (*kind, *attr) == (13, 7))
        .unwrap()
        .2[3] = F::P(4);
    let (ir, messages) = decode(&nodes);
    let [region] = ir.geometry.regions.as_slice() else {
        panic!("one solid region: {messages:#?}");
    };
    assert!(
        matches!(&region.unstated, Some(RegionUnstated::Unclassified { shells })
        if ids(shells) == ["sldprt:brep:shell#7", "sldprt:brep:shell#4"])
    );
}

#[test]
fn native_region_leaves_disjoint_shells_unclassified_with_all_faces_stated() {
    let (ir, messages) = decode(&two_sphere_shells(false));
    assert_eq!(ir.geometry.faces.len(), 2, "{messages:#?}");
    let [region] = ir.geometry.regions.as_slice() else {
        panic!("one stated solid region: {messages:#?}");
    };
    let Some(RegionUnstated::Unclassified { shells }) = &region.unstated else {
        panic!("unclassified shells: {region:#?}");
    };
    assert_eq!(ids(shells), ["sldprt:brep:shell#4", "sldprt:brep:shell#7"]);
}

/// A site whose only face is given up keeps its body, region, and shell, the
/// shell marked as not stating its faces.
#[test]
fn a_site_whose_every_face_is_lost_keeps_its_body_region_and_shell() {
    let (ir, messages) = decode(&loopless_nodes(50));
    let geometry = &ir.geometry;
    assert!(geometry.faces.is_empty());
    let [body] = geometry.bodies.as_slice() else {
        panic!("one body: {messages:#?}");
    };
    assert_eq!(body.kind, BodyKind::Solid);
    let [region] = geometry.regions.as_slice() else {
        panic!("one region");
    };
    let [shell] = geometry.shells.as_slice() else {
        panic!("one shell");
    };
    assert_eq!(shell.unstated, Some(Unstated::Members));
    assert_eq!(region.unstated, None);
    assert_eq!(ids(region.stated_shells()), [shell.id.as_str()]);
    assert!(
        messages.iter().any(|message| message.contains("face#5")),
        "{messages:#?}"
    );
}

/// A site whose only face is bridged and then given up keeps its body,
/// region, and shell.
#[test]
fn a_site_whose_bridged_face_is_given_up_keeps_its_body_region_and_shell() {
    let mut nodes = loopless_nodes(53);
    loop_naming(&mut nodes, 8, 5);
    let (ir, messages) = decode(&nodes);
    let geometry = &ir.geometry;
    assert!(geometry.faces.is_empty(), "{messages:#?}");
    assert_eq!(geometry.bodies.len(), 1, "{messages:#?}");
    assert_eq!(geometry.regions.len(), 1);
    let [shell] = geometry.shells.as_slice() else {
        panic!("one shell");
    };
    assert_eq!(shell.unstated, Some(Unstated::Members));
}

/// A region that gains two shells none of whose faces is stated does not
/// state which of them is the exterior.
#[test]
fn a_region_of_two_faceless_shells_leaves_them_unclassified() {
    let mut nodes = loopless_nodes(50);
    second_shell(&mut nodes, 50);
    let (ir, messages) = decode(&nodes);
    let geometry = &ir.geometry;
    assert!(geometry.faces.is_empty());
    let [region] = geometry.regions.as_slice() else {
        panic!("one region: {messages:#?}");
    };
    let Some(RegionUnstated::Unclassified { shells }) = &region.unstated else {
        panic!("unclassified shells: {region:#?}");
    };
    assert_eq!(ids(shells), ["sldprt:brep:shell#4", "sldprt:brep:shell#6"]);
    assert_eq!(ids(region.stated_shells()).len(), 2);
    assert!(geometry
        .shells
        .iter()
        .all(|shell| shell.unstated == Some(Unstated::Members)));
}

/// A region whose second shell holds no stated face, beside a shell that
/// does, does not state which of them is the exterior.
#[test]
fn a_region_with_a_faceless_shell_beside_a_stated_one_leaves_them_unclassified() {
    let mut nodes = loopless_nodes(53);
    second_shell(&mut nodes, 53);
    loop_naming(&mut nodes, 8, 7);
    let (ir, messages) = decode(&nodes);
    let geometry = &ir.geometry;
    let [face] = geometry.faces.as_slice() else {
        panic!("one face: {messages:#?}");
    };
    assert_eq!(face.id.as_str(), "sldprt:brep:face#5");
    let [region] = geometry.regions.as_slice() else {
        panic!("one region");
    };
    let Some(RegionUnstated::Unclassified { shells }) = &region.unstated else {
        panic!("unclassified shells: {region:#?}");
    };
    assert_eq!(ids(shells), ["sldprt:brep:shell#4", "sldprt:brep:shell#6"]);
    let marked: Vec<_> = geometry
        .shells
        .iter()
        .filter(|shell| shell.unstated == Some(Unstated::Members))
        .map(|shell| shell.id.as_str())
        .collect();
    assert_eq!(marked, ["sldprt:brep:shell#6"]);
}

/// A derived hierarchy whose BODY record no region names states the kind of
/// its faces and says that no body record names a region.
#[test]
fn a_derived_hierarchy_whose_body_no_region_names_says_so() {
    let mut nodes = loopless_nodes(53);
    nodes[0].2[14] = F::B(3);
    nodes[1].2[2] = F::P(9);
    let (ir, messages) = decode(&nodes);
    let [body] = ir.geometry.bodies.as_slice() else {
        panic!("one body: {messages:#?}");
    };
    assert_eq!(body.kind, BodyKind::Solid);
    assert!(
        messages
            .iter()
            .any(|message| message.starts_with("No body record names a region;")),
        "{messages:#?}"
    );
}
