// SPDX-License-Identifier: Apache-2.0
//! Bodies whose transforms the writer places into their geometry: geometry
//! and topology that bodies under different transforms share, and a copied
//! record that has no placed image.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::eval::decode::{pcurve_uv, surface_point_solved};
use cadmpeg_ir::examples::{add_document_root, unit_cube};
use cadmpeg_ir::geometry::pcurve::PcurveMetadata;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, SurfaceId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Coedge, Face};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::covered_uses::cube_with_pcurves;
use crate::test_support::decoded_with;
use crate::writer::placement::separated_bodies;
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

/// `ir` written, with the losses of the write and the document that reads
/// back with no loss.
fn written(ir: &CadIr) -> (Vec<LossNote>, CadIr) {
    let plan = IgesCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .expect("the document is written");
    let losses = plan.report().losses.clone();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the file is written");
    (losses, decoded_with(&bytes, &[]))
}

fn charged<'a>(losses: &'a [LossNote], code: &str) -> Vec<&'a str> {
    losses
        .iter()
        .filter(|loss| loss.code.to_string() == code)
        .map(|loss| loss.message.as_str())
        .collect()
}

/// Appends `suffix` to the key of each identity of a kind in `kinds` in
/// `value`.
fn renamed(value: &mut Value, kinds: &[&str], suffix: &str) {
    match value {
        Value::String(text) => {
            if let Some((head, _)) = text.split_once('#') {
                if kinds.iter().any(|kind| head.ends_with(&format!(":{kind}"))) {
                    text.push_str(suffix);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                renamed(item, kinds, suffix);
            }
        }
        Value::Object(fields) => {
            for field in fields.values_mut() {
                renamed(field, kinds, suffix);
            }
        }
        _ => {}
    }
}

/// `records` with `suffix` appended to each identity of a kind in `kinds`.
fn copied<T: Serialize + DeserializeOwned>(records: &[T], kinds: &[&str], suffix: &str) -> Vec<T> {
    let mut value = json::to_value(records).expect("the records serialize");
    renamed(&mut value, kinds, suffix);
    json::from_value(value).expect("the renamed records deserialize")
}

/// `records` with each identity of a kind in `kinds` renamed for the second
/// body.
fn instance<T: Serialize + DeserializeOwned>(records: &[T], kinds: &[&str]) -> Vec<T> {
    copied(records, kinds, "-instance")
}

/// The cube `ir` under `first`, and a second cube body under `second` on
/// the same records of each kind outside `kinds`. The radial ring of an edge
/// that both bodies use holds the coedges of the first body, then those of
/// the second.
fn shared_cubes(
    mut ir: CadIr,
    first: Option<Transform>,
    second: Option<Transform>,
    kinds: &[&str],
) -> CadIr {
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

/// The faces of the body `body` of `read`.
fn body_faces<'a>(read: &'a CadIr, body: &'a BodyId) -> impl Iterator<Item = &'a Face> {
    let geometry = &read.geometry;
    geometry
        .regions
        .iter()
        .filter(move |region| region.body == *body)
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
}

/// The positions of the vertices of each read-back body, by body.
fn body_vertices(read: &CadIr) -> Vec<Vec<[f64; 3]>> {
    let geometry = &read.geometry;
    geometry
        .bodies
        .iter()
        .map(|body| {
            let mut positions = Vec::new();
            for face in body_faces(read, &body.id) {
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
        unit_cube().expect("the example cube builds"),
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
        unit_cube().expect("the example cube builds"),
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

/// The support points at the ends of the parameter curves on the faces of
/// each read-back body, by body.
fn body_pcurve_ends(read: &CadIr) -> Vec<Vec<[f64; 3]>> {
    let geometry = &read.geometry;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("an evaluation context");
    geometry
        .bodies
        .iter()
        .map(|body| {
            let mut positions = Vec::new();
            for face in body_faces(read, &body.id) {
                let support = match &geometry
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface)
                    .expect("the face support reads back")
                    .geometry
                {
                    SurfaceGeometry::Solved(solved) => solved,
                    SurfaceGeometry::Procedural { cache, .. } => {
                        cache.as_ref().expect("the support reads back solved")
                    }
                };
                for coedge in geometry
                    .coedges
                    .iter()
                    .filter(|coedge| face.loops.stated().any(|loop_| *loop_ == coedge.owner_loop))
                {
                    for pcurve_use in &coedge.pcurves {
                        let pcurve = geometry
                            .pcurves
                            .iter()
                            .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                            .expect("the parameter curve reads back");
                        for parameter in pcurve
                            .parameter_range()
                            .expect("a bounded parameter curve")
                            .get()
                        {
                            let uv = pcurve_uv(&ctx, &pcurve.geometry, parameter)
                                .expect("a parameter point")
                                .get();
                            let point = surface_point_solved(&ctx, support, uv.u, uv.v)
                                .expect("a point on the support")
                                .get();
                            positions.push([point.x, point.y, point.z]);
                        }
                    }
                }
            }
            positions
        })
        .collect()
}

#[test]
fn a_parameter_curve_that_bodies_under_different_transforms_share_is_placed_once_for_each_body() {
    let ir = shared_cubes(
        cube_with_pcurves(false),
        None,
        Transform::affine([
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 3.0, -1.0],
        ]),
        &TOPOLOGY_KINDS,
    );
    let (losses, read) = written(&ir);
    for code in [
        "iges/writer.body-omitted",
        "iges/writer.face-omitted",
        "iges/writer.pcurve-omitted",
    ] {
        assert!(charged(&losses, code).is_empty(), "{losses:#?}");
    }
    assert_eq!(read.geometry.bodies.len(), 2);
    let corners = cube_corners();
    let placed = corners.iter().copied().map(stretched).collect::<Vec<_>>();
    let bodies = body_pcurve_ends(&read);
    for body in &bodies {
        assert_eq!(body.len(), 2 * 24, "{bodies:?}");
    }
    assert!(
        bodies.iter().any(|body| holds(body, &corners)),
        "no body's parameter curves end at the source corners: {bodies:?}"
    );
    assert!(
        bodies.iter().any(|body| holds(body, &placed)),
        "no body's parameter curves end at the placed corners: {bodies:?}"
    );
}

#[test]
fn the_ring_of_a_copied_edge_holds_only_the_coedges_of_its_body() {
    let ir = shared_cubes(
        unit_cube().expect("the example cube builds"),
        None,
        Transform::affine([
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 3.0, -1.0],
        ]),
        &FACE_KINDS,
    );
    let Ok(separated) = separated_bodies(&ir) else {
        panic!("the bodies separate");
    };
    let coedges = &separated.geometry.coedges;
    let instance = |coedge: &CoedgeId| coedge.as_str().ends_with("-instance");
    for coedge in coedges {
        let mut ring = vec![coedge];
        loop {
            let next = coedges
                .iter()
                .find(|record| record.id == ring[ring.len() - 1].radial_next)
                .expect("the radial ring resolves");
            if next.id == coedge.id {
                break;
            }
            assert!(
                ring.len() < coedges.len(),
                "the radial ring of {} never closes",
                coedge.id
            );
            ring.push(next);
        }
        assert_eq!(ring.len(), 2, "{ring:#?}");
        for member in &ring {
            assert_eq!(member.edge, coedge.edge, "{ring:#?}");
            assert_eq!(instance(&member.id), instance(&coedge.id), "{ring:#?}");
        }
    }
    let edges = coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(edges.len(), 24);
}

/// The support of the face of `coedge` in `geometry`.
fn coedge_support<'a>(geometry: &'a Topology, coedge: &Coedge) -> &'a SurfaceId {
    geometry
        .loops
        .iter()
        .find(|loop_| loop_.id == coedge.owner_loop)
        .and_then(|loop_| geometry.faces.iter().find(|face| face.id == loop_.face))
        .map(|face| &face.surface)
        .expect("the coedge face")
}

/// The cube with a parameter curve on each coedge that names the support of
/// the face of the coedge.
fn cube_with_supported_pcurves() -> CadIr {
    let mut ir = cube_with_pcurves(false);
    let supports = ir
        .geometry
        .coedges
        .iter()
        .flat_map(|coedge| {
            let support = coedge_support(&ir.geometry, coedge).clone();
            coedge
                .pcurves
                .iter()
                .map(move |pcurve_use| (pcurve_use.pcurve.clone(), support.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    for pcurve in &mut ir.geometry.pcurves {
        let PcurveMetadata::General { form } = &mut pcurve.metadata else {
            panic!("the cube states general parameter curves");
        };
        form.support_surface = Some(supports[&pcurve.id].clone());
    }
    ir
}

/// Asserts that each parameter curve of `geometry` names the support of the
/// face of each coedge that uses it.
fn assert_pcurves_name_their_face_supports(geometry: &Topology) {
    for coedge in &geometry.coedges {
        let support = coedge_support(geometry, coedge);
        for pcurve_use in &coedge.pcurves {
            let pcurve = geometry
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == pcurve_use.pcurve)
                .expect("the parameter curve");
            let PcurveMetadata::General { form } = &pcurve.metadata else {
                panic!("the copy keeps the general form");
            };
            assert_eq!(
                form.support_surface.as_ref(),
                Some(support),
                "{} names another support than the face of {}",
                pcurve.id,
                coedge.id
            );
        }
    }
}

#[test]
fn a_copied_parameter_curve_names_the_copy_of_its_support() {
    let ir = shared_cubes(
        cube_with_supported_pcurves(),
        None,
        Transform::affine([
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 3.0, -1.0],
        ]),
        &TOPOLOGY_KINDS,
    );
    let Ok(separated) = separated_bodies(&ir) else {
        panic!("the bodies separate");
    };
    let geometry = &separated.geometry;
    assert_pcurves_name_their_face_supports(geometry);
    let mut bodies = [BTreeSet::new(), BTreeSet::new()];
    for coedge in &geometry.coedges {
        let support = coedge_support(geometry, coedge);
        bodies[usize::from(coedge.id.as_str().ends_with("-instance"))].insert(support.clone());
    }
    assert_eq!(bodies[0].len(), 6);
    assert!(bodies[0].is_disjoint(&bodies[1]), "{bodies:#?}");
    let (losses, _) = written(&ir);
    for code in [
        "iges/writer.body-omitted",
        "iges/writer.face-omitted",
        "iges/writer.pcurve-omitted",
    ] {
        assert!(charged(&losses, code).is_empty(), "{losses:#?}");
    }
}

#[test]
fn a_parameter_curve_on_a_support_that_a_body_under_another_transform_owns_first_is_copied_with_it()
{
    let mut ir = cube_with_supported_pcurves();
    let source = ir.geometry.clone();
    for suffix in ["-instance", "-placed"] {
        let geometry = &mut ir.geometry;
        let mut bodies = copied(&source.bodies, &TOPOLOGY_KINDS, suffix);
        bodies[0].transform = Transform::affine([
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 3.0, -1.0],
        ]);
        geometry.bodies.extend(bodies);
        geometry
            .regions
            .extend(copied(&source.regions, &TOPOLOGY_KINDS, suffix));
        geometry
            .shells
            .extend(copied(&source.shells, &TOPOLOGY_KINDS, suffix));
        geometry
            .faces
            .extend(copied(&source.faces, &TOPOLOGY_KINDS, suffix));
        geometry
            .loops
            .extend(copied(&source.loops, &TOPOLOGY_KINDS, suffix));
        geometry
            .coedges
            .extend(copied(&source.coedges, &TOPOLOGY_KINDS, suffix));
        geometry
            .edges
            .extend(copied(&source.edges, &TOPOLOGY_KINDS, suffix));
        geometry
            .vertices
            .extend(copied(&source.vertices, &TOPOLOGY_KINDS, suffix));
    }
    for coedge in &mut ir.geometry.coedges[..source.coedges.len()] {
        coedge.pcurves.clear();
    }
    ir.finalize();
    let order = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.transform.is_some())
        .collect::<Vec<_>>();
    assert_eq!(order, [false, true, true], "{:#?}", ir.geometry.bodies);
    let Ok(separated) = separated_bodies(&ir) else {
        panic!("the bodies separate");
    };
    let geometry = &separated.geometry;
    assert_pcurves_name_their_face_supports(geometry);
    let used = |suffix: &str| {
        geometry
            .coedges
            .iter()
            .filter(|coedge| coedge.id.as_str().ends_with(suffix))
            .flat_map(|coedge| coedge.pcurves.iter().map(|pcurve_use| &pcurve_use.pcurve))
            .collect::<BTreeSet<_>>()
    };
    let sources = ir
        .geometry
        .pcurves
        .iter()
        .map(|pcurve| &pcurve.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(used("-instance"), sources);
    let copies = used("-placed");
    assert_eq!(copies.len(), sources.len());
    assert!(copies.is_disjoint(&sources), "{copies:#?}");
    let (losses, read) = written(&ir);
    for code in [
        "iges/writer.body-omitted",
        "iges/writer.face-omitted",
        "iges/writer.pcurve-omitted",
    ] {
        assert!(charged(&losses, code).is_empty(), "{losses:#?}");
    }
    assert_eq!(read.geometry.bodies.len(), 3);
    let placed = cube_corners()
        .into_iter()
        .map(stretched)
        .collect::<Vec<_>>();
    let bodies = body_pcurve_ends(&read)
        .into_iter()
        .filter(|body| !body.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    for body in &bodies {
        assert!(
            holds(body, &placed),
            "a stretched body's parameter curves end off the placed corners: {body:?}"
        );
    }
}

/// The example cube with a copy of its first face, the fin, in the same
/// shell on the same edges: each edge of the fin has three coedges.
fn cube_with_a_fin() -> CadIr {
    let mut ir = unit_cube().expect("the example cube builds");
    let geometry = &mut ir.geometry;
    let face = geometry.faces[0].clone();
    let loops = geometry
        .loops
        .iter()
        .filter(|loop_| loop_.face == face.id)
        .cloned()
        .collect::<Vec<_>>();
    let coedges = geometry
        .coedges
        .iter()
        .filter(|coedge| loops.iter().any(|loop_| loop_.id == coedge.owner_loop))
        .cloned()
        .collect::<Vec<_>>();
    let kinds = ["face", "loop", "coedge"];
    geometry
        .faces
        .extend(copied(std::slice::from_ref(&face), &kinds, "-fin"));
    geometry.loops.extend(copied(&loops, &kinds, "-fin"));
    geometry.coedges.extend(copied(&coedges, &kinds, "-fin"));
    ir.finalize();
    ir
}

#[test]
fn the_ring_of_a_copied_edge_keeps_the_source_order_of_the_coedges_of_its_body() {
    let mut ir = shared_cubes(
        cube_with_a_fin(),
        None,
        Transform::affine([
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 3.0, -1.0],
        ]),
        &FACE_KINDS,
    );
    // Each source ring runs in descending identity order.
    let mut rings = BTreeMap::<EdgeId, Vec<CoedgeId>>::new();
    for coedge in &ir.geometry.coedges {
        rings
            .entry(coedge.edge.clone())
            .or_default()
            .push(coedge.id.clone());
    }
    for ring in rings.values_mut() {
        ring.sort_by(|first, second| second.cmp(first));
    }
    let next = rings
        .values()
        .flat_map(|ring| {
            ring.iter()
                .zip(ring.iter().cycle().skip(1))
                .map(|(coedge, next)| (coedge.clone(), next.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    for coedge in &mut ir.geometry.coedges {
        coedge.radial_next = next[&coedge.id].clone();
    }
    let Ok(separated) = separated_bodies(&ir) else {
        panic!("the bodies separate");
    };
    let instance = |coedge: &CoedgeId| coedge.as_str().ends_with("-instance");
    let mut fins = 0;
    for coedge in &separated.geometry.coedges {
        let source = ir
            .geometry
            .coedges
            .iter()
            .find(|source| source.id == coedge.id)
            .expect("the coedge keeps its identity");
        let ring = rings[&source.edge]
            .iter()
            .filter(|member| instance(member) == instance(&coedge.id))
            .collect::<Vec<_>>();
        let position = ring
            .iter()
            .position(|member| **member == coedge.id)
            .expect("the coedge is in its ring");
        assert_eq!(
            coedge.radial_next,
            *ring[(position + 1) % ring.len()],
            "{ring:#?}"
        );
        if ring.len() == 3 {
            fins += 1;
        }
    }
    assert_eq!(fins, 2 * 4 * 3);
}
