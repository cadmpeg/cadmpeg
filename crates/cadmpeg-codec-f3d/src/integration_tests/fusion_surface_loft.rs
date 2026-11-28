// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of a new-body surface Loft.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

fn assert_surface_loft(bytes: &[u8]) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    let inferred = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == crate::loss::F3dLossCode::FeatureLoftSolidInferred.kind())
        .collect::<Vec<_>>();
    assert_eq!(inferred.len(), 1);
    assert!(
        inferred[0]
            .message
            .ends_with("projects solid false because its 1 output bodies are sheet bodies at its own history state (DR-331)."),
        "{}",
        inferred[0].message
    );
    let ir = cadmpeg_test_support::wire::in_memory_json(result.ir());
    let bodies = ir["geometry"]["bodies"].as_array().expect("bodies");
    let mut lofts = ir["model"]["features"]
        .as_array()
        .expect("features")
        .iter()
        .filter(|feature| feature["definition"]["value"]["kind"] == "loft");
    let loft = lofts.next().expect("one Loft");
    assert!(lofts.next().is_none(), "one Loft");
    let outputs = loft["outputs"].as_array().expect("outputs");
    assert_eq!(outputs.len(), 1);
    let output = bodies
        .iter()
        .find(|body| body["id"] == outputs[0])
        .expect("output body");
    assert_eq!(output["kind"], "sheet");
    let definition = &loft["definition"]["value"]["value"];
    assert_eq!(definition["op"], "new_body");
    assert_eq!(definition["solid"], false);
}

#[test]
fn a_new_body_loft_with_a_sheet_output_is_a_surface_loft() {
    assert_surface_loft(include_bytes!(
        "../../../../corpus/f3d/reverse_normal_loft_control.f3d"
    ));
    assert_surface_loft(include_bytes!(
        "../../../../corpus/f3d/reverse_normal_loft.f3d"
    ));
}

#[test]
fn a_loft_pass_refusal_keeps_the_decode_and_charges_a_loss() {
    let bytes = include_bytes!("../../../../corpus/f3d/reverse_normal_loft.f3d");
    let full = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    crate::history::history_bodies::inject_loft_pass_refusal();
    let contained = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("a Loft pass refusal is contained");

    assert_eq!(contained.ir(), full.ir());
    let messages = |code: crate::loss::F3dLossCode| {
        contained
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == code.kind())
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>()
    };
    let bounded = messages(crate::loss::F3dLossCode::FeatureHistoryBodyBounded);
    assert_eq!(bounded.len(), 1, "{bounded:?}");
    assert!(
        bounded[0].contains("output bodies at its own history state exceed the Loft pass budget"),
        "{}",
        bounded[0]
    );
    let inferred = messages(crate::loss::F3dLossCode::FeatureLoftSolidInferred);
    assert_eq!(inferred.len(), 1);
    assert!(
        inferred[0].ends_with(
            "projects solid false because its 1 output bodies are sheet bodies in the final model (DR-331)."
        ),
        "{}",
        inferred[0]
    );
}

#[test]
fn loft_evidence_leaves_the_required_design_timeline_its_entities() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    use cadmpeg_core::CodecError;

    let bytes = include_bytes!("../../../../corpus/f3d/reverse_normal_loft.f3d");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        bytes,
        &arena,
        &DecodePolicy::default(),
        DecodeDepth::Design,
    )
    .expect("fixture root");
    crate::history::history_bodies::inject_loft_pass_refusal();
    let required = crate::decode::decode(&ctx, root).expect("timeline without optional evidence");
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_entities(u64::MAX, "measure charge")
    else {
        panic!("session use is measurable");
    };
    assert!(!required.ir.design.containers.is_empty());
    let mut policy = DecodePolicy::default();
    policy.limits.max_entities = limit.used;
    let (bounded, root) =
        DecodeContext::from_root_bytes_at(bytes, &arena, &policy, DecodeDepth::Design)
            .expect("bounded fixture root");
    let decoded = crate::decode::decode(&bounded, root)
        .expect("the optional Loft pass leaves enough entities for the timeline");
    assert_eq!(decoded.ir, required.ir);
    assert!(decoded.body.losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::FeatureLoftSolidInferred.kind()
            && loss.message.contains("at its own history state")
    }));
}
