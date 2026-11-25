// SPDX-License-Identifier: Apache-2.0
//! Bodies whose transforms the writer places into their geometry: geometry
//! and topology that bodies under different transforms share, and a copied
//! record that has no placed image.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::examples::{add_document_root, unit_cube};
use cadmpeg_ir::ids::CoedgeId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::{IgesCodec, IgesVersion};

/// The kinds of record that a second body on shared geometry renames.
const TOPOLOGY_KINDS: [&str; 8] = [
    "body", "region", "shell", "face", "loop", "coedge", "edge", "vertex",
];

/// The kinds of record that a second body on shared edges and vertices
/// renames.
const FACE_KINDS: [&str; 6] = ["body", "region", "shell", "face", "loop", "coedge"];

/// Bound on the distance between a read-back point and its expected place.
const PLACED: f64 = 1.0e-9;

fn written(ir: &CadIr) -> (Vec<LossNote>, CadIr) {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None).with_target(TargetRequest::Explicit(
                IgesVersion::V5_3.descriptor().id.as_str(),
            )),
        )
        .expect("the document is written");
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the file is written");
    let read = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written file decodes")
        .ir()
        .clone();
    (losses, read)
}

fn charged<'a>(losses: &'a [LossNote], code: &str) -> Vec<&'a str> {
    losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .map(|loss| loss.message.as_str())
        .collect()
}

/// Appends `-instance` to the key of each identity of a kind in `kinds` in
/// `value`.
fn renamed(value: &mut Value, kinds: &[&str]) {
    match value {
        Value::String(text) => {
            if let Some((head, _)) = text.split_once('#') {
                if kinds.iter().any(|kind| head.ends_with(&format!(":{kind}"))) {
                    text.push_str("-instance");
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                renamed(item, kinds);
            }
        }
        Value::Object(fields) => {
            for field in fields.values_mut() {
                renamed(field, kinds);
            }
        }
        _ => {}
    }
}

/// `records` with each identity of a kind in `kinds` renamed for the second
/// body.
fn instance<T: Serialize + DeserializeOwned>(records: &[T], kinds: &[&str]) -> Vec<T> {
    let mut value = serde_json::to_value(records).expect("the records serialize");
    renamed(&mut value, kinds);
    serde_json::from_value(value).expect("the renamed records deserialize")
}

/// The example cube under `first`, and a second cube body under `second` on
/// the same records of each kind outside `kinds`. The radial ring of an edge
/// that both bodies use holds the coedges of the first body, then those of
/// the second.
fn shared_cubes(first: Option<Transform>, second: Option<Transform>, kinds: &[&str]) -> CadIr {
    let mut ir = unit_cube().expect("the example cube builds");
    let geometry = &mut ir.geometry;
    geometry.bodies[0].transform = first;
    let mut bodies = instance(&geometry.bodies, kinds);
    bodies[0].transform = second;
    let regions = instance(&geometry.regions, kinds);
    let shells = instance(&geometry.shells, kinds);
    let faces = instance(&geometry.faces, kinds);
    let loops = instance(&geometry.loops, kinds);
    let coedges = instance(&geometry.coedges, kinds);
    let edges = instance(&geometry.edges, kinds);
    let vertices = instance(&geometry.vertices, kinds);
    let shared_edges = !kinds.contains(&"edge");
    geometry.bodies.extend(bodies);
    geometry.regions.extend(regions);
    geometry.shells.extend(shells);
    geometry.faces.extend(faces);
    geometry.loops.extend(loops);
    geometry.coedges.extend(coedges);
    if shared_edges {
        let mut rings = BTreeMap::<String, Vec<String>>::new();
        for coedge in &geometry.coedges {
            rings
                .entry(coedge.edge.as_str().to_owned())
                .or_default()
                .push(coedge.id.as_str().to_owned());
        }
        let next = rings
            .values()
            .flat_map(|ring| {
                ring.iter()
                    .zip(ring.iter().cycle().skip(1))
                    .map(|(coedge, next)| (coedge.clone(), next.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        for coedge in &mut geometry.coedges {
            coedge.radial_next = CoedgeId::try_from(next[coedge.id.as_str()].clone())
                .expect("the coedge identity is valid");
        }
    } else {
        geometry.edges.extend(edges);
    }
    if kinds.contains(&"vertex") {
        geometry.vertices.extend(vertices);
    }
    add_document_root(&mut ir);
    ir.finalize();
    ir
}

fn cube_corners() -> Vec<[f64; 3]> {
    unit_cube()
        .expect("the example cube builds")
        .geometry
        .points
        .iter()
        .map(|point| {
            let position = point.position().get();
            [position.x, position.y, position.z]
        })
        .collect()
}

/// The positions of the vertices of each read-back body, by body.
fn body_vertices(read: &CadIr) -> Vec<Vec<[f64; 3]>> {
    let geometry = &read.geometry;
    geometry
        .bodies
        .iter()
        .map(|body| {
            let faces = geometry
                .regions
                .iter()
                .filter(|region| region.body == body.id)
                .flat_map(|region| {
                    geometry
                        .shells
                        .iter()
                        .filter(move |shell| shell.region == region.id)
                })
                .flat_map(|shell| {
                    geometry
                        .faces
                        .iter()
                        .filter(move |face| face.shell == shell.id)
                })
                .collect::<Vec<_>>();
            let mut positions = Vec::new();
            for face in faces {
                for loop_ in geometry
                    .loops
                    .iter()
                    .filter(|loop_| face.loops.stated().any(|id| *id == loop_.id))
                {
                    let ends = loop_
                        .coedges()
                        .iter()
                        .filter_map(|coedge| {
                            geometry.coedges.iter().find(|record| record.id == *coedge)
                        })
                        .filter_map(|coedge| {
                            geometry.edges.iter().find(|edge| edge.id == coedge.edge)
                        })
                        .flat_map(cadmpeg_ir::topology::Edge::vertices)
                        .collect::<Vec<_>>();
                    for vertex in ends {
                        let vertex = geometry
                            .vertices
                            .iter()
                            .find(|record| record.id == *vertex)
                            .expect("the edge vertex reads back");
                        let point = geometry
                            .points
                            .iter()
                            .find(|point| point.id == vertex.point)
                            .expect("the vertex point reads back")
                            .position()
                            .get();
                        positions.push([point.x, point.y, point.z]);
                    }
                }
            }
            positions
        })
        .collect()
}

fn holds(positions: &[[f64; 3]], expected: &[[f64; 3]]) -> bool {
    positions.len() >= expected.len()
        && expected.iter().all(|corner| {
            positions.iter().any(|position| {
                position
                    .iter()
                    .zip(corner)
                    .all(|(actual, expected)| (actual - expected).abs() <= PLACED)
            })
        })
        && positions.iter().all(|position| {
            expected.iter().any(|corner| {
                position
                    .iter()
                    .zip(corner)
                    .all(|(actual, expected)| (actual - expected).abs() <= PLACED)
            })
        })
}

fn stretched(point: [f64; 3]) -> [f64; 3] {
    [
        2.0f64.mul_add(point[0], 1.0),
        point[1],
        3.0f64.mul_add(point[2], -1.0),
    ]
}

/// Writes two cubes that share the records of each kind outside `kinds`, the
/// second stretched, and reads back each body at its own corners.
fn placed_once_for_each_body(kinds: &[&str]) {
    let ir = shared_cubes(
        None,
        Transform::affine([
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 3.0, -1.0],
        ]),
        kinds,
    );
    let (losses, read) = written(&ir);
    for code in ["iges/writer.body-omitted", "iges/writer.face-omitted"] {
        assert!(charged(&losses, code).is_empty(), "{losses:#?}");
    }
    assert_eq!(read.geometry.bodies.len(), 2);
    assert_eq!(read.geometry.faces.len(), 12);
    let corners = cube_corners();
    let placed = corners.iter().copied().map(stretched).collect::<Vec<_>>();
    let bodies = body_vertices(&read);
    assert!(
        bodies.iter().any(|body| holds(body, &corners)),
        "no body reads back at the source corners: {bodies:?}"
    );
    assert!(
        bodies.iter().any(|body| holds(body, &placed)),
        "no body reads back at the placed corners: {bodies:?}"
    );
}

/// Writes two cubes that share the records of each kind outside `kinds`,
/// the second under a transform that has no finite image of five of its
/// faces, and reads back the first body and the sixth face of the second.
fn drops_only_instance_faces(kinds: &[&str]) {
    // The image of each cube corner at x = 10 leaves the finite range; the
    // instance face on the plane x = 0 keeps a finite image.
    let ir = shared_cubes(
        None,
        Transform::affine([
            [1.0e308, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]),
        kinds,
    );
    let (losses, read) = written(&ir);
    let omitted = charged(&losses, "iges/writer.face-omitted");
    assert!(!omitted.is_empty(), "{losses:#?}");
    for message in &omitted {
        assert!(
            message.starts_with("face synthetic:cube:face#") && message.contains("-instance "),
            "a face of the source body is dropped: {message}"
        );
        assert!(
            message.contains("iges:writer:"),
            "the message names no copied record: {message}"
        );
    }
    assert_eq!(omitted.len(), 5, "{omitted:#?}");
    assert!(
        charged(&losses, "export:output.empty").is_empty(),
        "{losses:#?}"
    );
    let bodies = body_vertices(&read);
    assert!(
        bodies.iter().any(|body| holds(body, &cube_corners())),
        "the source body does not read back whole: {bodies:?}"
    );
    assert_eq!(read.geometry.faces.len(), 7);
    let placed_face = [0.0, 10.0]
        .into_iter()
        .flat_map(|y| [0.0, 10.0].map(|z| [0.0, y, z]))
        .collect::<Vec<_>>();
    assert!(
        bodies.iter().any(|body| holds(body, &placed_face)),
        "the instance face on x = 0 does not read back: {bodies:?}"
    );
}

#[test]
fn geometry_that_bodies_under_different_transforms_share_is_placed_once_for_each_body() {
    placed_once_for_each_body(&TOPOLOGY_KINDS);
}

#[test]
fn edges_and_vertices_that_bodies_under_different_transforms_share_are_placed_once_for_each_body() {
    placed_once_for_each_body(&FACE_KINDS);
}

#[test]
fn a_copied_record_with_no_placed_image_drops_only_the_faces_of_its_body() {
    drops_only_instance_faces(&TOPOLOGY_KINDS);
}

#[test]
fn a_copied_edge_or_vertex_with_no_placed_image_drops_only_the_faces_of_its_body() {
    drops_only_instance_faces(&FACE_KINDS);
}
