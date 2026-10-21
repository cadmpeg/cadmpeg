// SPDX-License-Identifier: Apache-2.0
//! Complete records and one global mapping, rather than counts, justify Written.

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::write::{target::TargetRequest, ArenaDisposition, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::schema::EntityKind;

use crate::RhinoCodec;

const BREP: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/fixtures/brep.3dm"
));
const SEAM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/fixtures/brep_singular_seam.3dm"
));

#[test]
fn synthesized_brep_claims_real_written_topology() {
    let source = RhinoCodec
        .decode(&mut Cursor::new(BREP), &DecodeOptions::default())
        .unwrap();
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(source.ir(), None),
            TargetRequest::Explicit("rhino:archive-80"),
        )
        .unwrap();
    for kind in [
        EntityKind::Region,
        EntityKind::Shell,
        EntityKind::Face,
        EntityKind::Loop,
        EntityKind::Coedge,
        EntityKind::Edge,
        EntityKind::Vertex,
        EntityKind::Pcurve,
    ] {
        assert_eq!(
            plan.report().coverage.disposition(kind),
            ArenaDisposition::Written
        );
    }
    // The output assigns default layer/color metadata absent from this source.
    assert_eq!(
        plan.report().coverage.disposition(EntityKind::Point),
        ArenaDisposition::Reported
    );
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).unwrap();
    let output = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, source.ir(), output.ir());
}

#[test]
fn unsupported_brep_does_not_claim_omitted_topology() {
    let source = RhinoCodec
        .decode(&mut Cursor::new(SEAM), &DecodeOptions::default())
        .unwrap();
    assert!(!source.ir().model.faces.is_empty());
    let plan = RhinoCodec
        .plan(
            EncodeInput::new(source.ir(), None),
            TargetRequest::Explicit("rhino:archive-80"),
        )
        .unwrap();
    assert_eq!(
        plan.report().coverage.disposition(EntityKind::Face),
        ArenaDisposition::Reported
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| { loss.severity == cadmpeg_ir::report::Severity::Error }));
}

#[test]
fn same_counts_do_not_prove_references_or_names_equal() {
    let source = RhinoCodec
        .decode(&mut Cursor::new(BREP), &DecodeOptions::default())
        .unwrap();
    let left = source.ir();
    let mut right = left.clone();
    right.model.vertices[0].point = right.model.vertices[1].point.clone();
    let ids = super::ordinal_identities(left, &right).unwrap();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert!(!super::agrees(
        &ctx,
        &left.model.vertices,
        &right.model.vertices,
        &ids
    ));
    right = left.clone();
    right.model.bodies[0].name = Some(left.model.bodies[0].id.to_string());
    let ids = super::ordinal_identities(left, &right).unwrap();
    assert!(!super::agrees(
        &ctx,
        &left.model.bodies,
        &right.model.bodies,
        &ids
    ));
}

#[test]
fn numeric_looking_display_names_remain_literal_content() {
    let left =
        serde_json::json!({"name": "Part 1.0000000000001", "tolerance": 1.000_000_000_000_1});
    let right =
        serde_json::json!({"name": "Part 1.0000000000002", "tolerance": 1.000_000_000_000_2});
    assert!(cadmpeg_ir::compare::values_agree(&left, &right).is_ok());
    assert!(!super::same_values(&left, &right));
    assert!(super::same_values(
        &serde_json::json!({"tolerance": 1.000_000_000_000_1}),
        &serde_json::json!({"tolerance": 1.000_000_000_000_2}),
    ));
}
