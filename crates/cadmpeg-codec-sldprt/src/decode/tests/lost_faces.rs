// SPDX-License-Identifier: Apache-2.0
//! Decode tests of the body, region, and shell records whose faces are lost
//! (CADIR §3.3, §5.6).
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::topology::{BodyKind, RegionUnstated, Unstated};
use cadmpeg_ir::CadIr;

use crate::test_support::container::{make_block, outer_header};
use crate::test_support::parasolid::sequence::{face_fields, loopless_nodes, sequence_of, F};
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
