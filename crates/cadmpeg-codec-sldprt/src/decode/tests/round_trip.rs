// SPDX-License-Identifier: Apache-2.0
//! Decode/encode equivariance and fixpoint tests.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use crate::test_support::*;
use crate::SldprtCodec;

#[test]
fn decode_encode_is_equivariant_under_rigid_motion() {
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::transform::Transform;

    let motions = [
        (
            [
                [0.0, -1.0, 0.0, 10.0],
                [1.0, 0.0, 0.0, 20.0],
                [0.0, 0.0, 1.0, 30.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            (|p: Point3| Point3::new(-p.y + 10.0, p.x + 20.0, p.z + 30.0)) as fn(Point3) -> Point3,
        ),
        (
            [
                [1.0, 0.0, 0.0, -5.0],
                [0.0, 0.0, -1.0, 7.0],
                [0.0, 1.0, 0.0, 3.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            |p: Point3| Point3::new(p.x - 5.0, -p.z + 7.0, p.y + 3.0),
        ),
    ];

    // The `.sldprt` semantic writer refuses a body or face name without a
    // material, so strip the labels the round trip does not exercise here.
    let prepare = |ir: &mut cadmpeg_ir::document::CadIr| {
        ir.model.bodies[0].name = None;
        ir.model.faces.iter_mut().for_each(|face| face.name = None);
        ir.model
            .edges
            .iter_mut()
            .for_each(|edge| edge.param_range = None);
    };

    let mut base = cadmpeg_ir::examples::unit_cube();
    prepare(&mut base);
    base.model.bodies[0].transform = None;
    let mut base_bytes = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &base,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut base_bytes))
        .unwrap();
    let reference = SldprtCodec
        .decode(&mut Cursor::new(base_bytes), &DecodeOptions::default())
        .unwrap();
    let reference_points: Vec<Point3> = reference
        .ir()
        .model
        .points
        .iter()
        .map(|point| point.position)
        .collect();

    for (rows, apply) in motions {
        let mut moved = cadmpeg_ir::examples::unit_cube();
        prepare(&mut moved);
        moved.model.bodies[0].transform = Some(Transform { rows });
        let mut bytes = Vec::new();
        SldprtCodec
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &moved,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .unwrap();
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();

        for reference_point in &reference_points {
            let expected = apply(*reference_point);
            assert!(
                decoded.ir().model.points.iter().any(|point| {
                    (point.position.x - expected.x).abs() < 1e-9
                        && (point.position.y - expected.y).abs() < 1e-9
                        && (point.position.z - expected.z).abs() < 1e-9
                }),
                "rigid motion not preserved for point {reference_point:?}"
            );
        }
        assert!(decoded
            .ir()
            .model
            .bodies
            .iter()
            .all(|body| body.transform.is_none()));
    }
}

#[test]
fn decode_encode_decode_reaches_fixpoint() {
    let fixture = sldprt_with_body_and_history(&triangle_body());

    let first = SldprtCodec
        .decode(&mut Cursor::new(fixture), &DecodeOptions::default())
        .expect("first decode");
    assert!(first.report().geometry_transferred);

    let mut reencoded = Vec::new();
    SldprtCodec
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: first.ir(),
            fidelity: Some(first.source_fidelity()),
        })
        .and_then(|plan| plan.write_to(&mut reencoded))
        .expect("re-encode");

    let second = SldprtCodec
        .decode(&mut Cursor::new(reencoded), &DecodeOptions::default())
        .expect("second decode");

    assert_eq!(
        first.ir().model.points,
        second.ir().model.points,
        "points diverged at the fixpoint"
    );
    assert_eq!(
        first.ir().model.surfaces,
        second.ir().model.surfaces,
        "surfaces diverged at the fixpoint"
    );
    assert_eq!(
        first.ir().model.faces,
        second.ir().model.faces,
        "faces diverged at the fixpoint"
    );
    assert_eq!(
        first.ir().model.edges,
        second.ir().model.edges,
        "edges diverged at the fixpoint"
    );
    assert_eq!(
        first.ir().model.coedges,
        second.ir().model.coedges,
        "coedges diverged at the fixpoint"
    );
    assert_eq!(
        first.report().geometry_transferred,
        second.report().geometry_transferred,
        "geometry-transferred flag diverged at the fixpoint"
    );
}

/// Metamorphic property: a rigid translation of the input produces the same
/// rigid translation of the decoded output (equivariance), and topology is
/// invariant. Reader and writer cannot silently drop or reorient geometry
/// without breaking one of these relations.
#[test]
fn decode_is_equivariant_under_rigid_translation() {
    let base = source_less_cube();
    let t = [3.5, -7.25, 11.0];

    let mut moved = base.clone();
    translate_model(&mut moved, t);

    let base_out = encode_decode(&base);
    let moved_out = encode_decode(&moved);

    // Topology is invariant under a rigid motion.
    assert_eq!(base_out.model.faces.len(), moved_out.model.faces.len());
    assert_eq!(base_out.model.edges.len(), moved_out.model.edges.len());
    assert_eq!(
        base_out.model.vertices.len(),
        moved_out.model.vertices.len()
    );

    // Point positions of the moved decode equal the base decode shifted by t.
    let base_positions = sorted_point_positions(&base_out);
    let moved_positions = sorted_point_positions(&moved_out);
    assert_eq!(base_positions.len(), moved_positions.len());
    for (b, m) in base_positions.iter().zip(&moved_positions) {
        for axis in 0..3 {
            assert!(
                (b[axis] + t[axis] - m[axis]).abs() < 1e-6,
                "axis {axis}: {b:?} + {t:?} != {m:?}"
            );
        }
    }
}

/// Decode → encode → decode fixpoint: once through the writer, a source-less
/// model reaches a fixed point whose semantic hash and topology no longer
/// change. Paired with the value golden below so a shared reader/writer
/// misconception cannot hide behind a self-consistent round trip.
#[test]
fn source_less_cube_reaches_encode_decode_fixpoint() {
    let first = encode_decode_result(&source_less_cube());
    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(first.ir(), first.source_fidelity(), &mut encoded)
        .unwrap();
    let second = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap()
        .into_parts()
        .0;

    let first_hash = crate::decode::document_local_sha256(first.ir());
    let second_hash = crate::decode::document_local_sha256(&second);
    assert_eq!(first_hash, second_hash, "round trip is not a fixed point");

    // Value golden: the cube's record families and counts, asserted directly.
    assert_eq!(first.ir().model.bodies.len(), 1);
    assert_eq!(first.ir().model.faces.len(), 6);
    assert_eq!(first.ir().model.edges.len(), 12);
    assert_eq!(first.ir().model.vertices.len(), 8);
    assert_eq!(first.ir().model.coedges.len(), 24);
    assert_eq!(first.ir().model.loops.len(), 6);
    assert_eq!(
        sorted_point_positions(first.ir()),
        sorted_point_positions(&second)
    );
}
