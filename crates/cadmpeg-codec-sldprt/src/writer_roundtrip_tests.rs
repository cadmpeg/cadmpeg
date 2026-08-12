// SPDX-License-Identifier: Apache-2.0
//! Semantic-write round-trip pins for the SLDPRT bake pipeline.
//!
//! Pre-transform rejection coverage stays in `tests.rs`
//! (`semantic_writer_rejects_invalid_ir_without_panicking`).

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::transform::Transform;

use crate::tests::{sldprt_with_body, triangle_body};
use crate::SldprtCodec;

#[test]
fn mutated_semantic_write_round_trips() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .expect("triangle fixture should decode");
    decoded.ir_mut().model.points[0].position.z += 1.0;
    let expected_z = decoded.ir().model.points[0].position.z;
    let expected_bodies = decoded.ir().model.bodies.len();
    let expected_faces = decoded.ir().model.faces.len();

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .expect("mutated triangle should write");
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("written triangle should decode");
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert_eq!(round_trip.ir().model.bodies.len(), expected_bodies);
    assert_eq!(round_trip.ir().model.faces.len(), expected_faces);
    assert!(
        floats_agree(round_trip.ir().model.points[0].position.z, expected_z),
        "mutated z drifted: got {} expected {}",
        round_trip.ir().model.points[0].position.z,
        expected_z
    );
}

#[test]
fn bake_transform_is_applied_and_output_stays_valid() {
    let mut decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(&triangle_body())),
            &DecodeOptions::default(),
        )
        .expect("triangle fixture should decode");
    let original_x = decoded.ir().model.points[0].position.x;
    decoded.ir_mut().model.bodies[0].transform = Some(Transform {
        rows: [
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    });

    let mut encoded = Vec::new();
    SldprtCodec
        .write_preserved_with_source_fidelity(decoded.ir(), decoded.source_fidelity(), &mut encoded)
        .expect("translated triangle should write");
    let round_trip = SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("written translated triangle should decode");
    let validation = cadmpeg_ir::validate_neutral(round_trip.ir(), Vec::new());
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert!(
        floats_agree(
            round_trip.ir().model.points[0].position.x,
            original_x + 10.0
        ),
        "baked translation drifted: got {} expected {}",
        round_trip.ir().model.points[0].position.x,
        original_x + 10.0
    );
}
