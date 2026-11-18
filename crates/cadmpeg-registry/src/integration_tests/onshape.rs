// SPDX-License-Identifier: Apache-2.0
//! Registry construction and nested STEP composition.
use crate::{Format, InputCatalog};
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_test_support::zip_fixture::{archive, RawEntry};
use std::io::Cursor;

#[test]
fn onshape_is_registered_as_a_reader_with_composed_step_identity() {
    let mut step = Vec::new();
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    cadmpeg_codec_step::StepCodec::default()
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit("step:ap214"),
        )
        .unwrap()
        .write_to(&mut step)
        .unwrap();
    let bytes = archive(&[
        RawEntry::stored("manifest.json", br#"{"schema":"dutchman.onshape-bundle/1","document":{"microversion":"snapshot"},"elements":[{"id":"studio","type":"PARTSTUDIO"}]}"#),
        RawEntry::stored("partstudios/studio/features.json", br#"{"features":[{"featureId":"inactive","featureType":"fixture","suppressed":true}],"sourceMicroversion":"snapshot"}"#),
        RawEntry::stored("partstudios/studio/bodydetails/index.json", b"[]"),
        RawEntry::stored("partstudios/studio/export.step", &step),
    ]);
    let catalog = InputCatalog::with_builtins();
    let descriptor = catalog
        .descriptors()
        .find(|d| d.format_id().as_str() == "onshape")
        .unwrap();
    assert_eq!(descriptor.extensions(), ["onshape-bundle"]);
    assert!(crate::forced_input("onshape").unwrap().is_some());
    assert!(Format::from_name("onshape").unwrap().is_none());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let crate::DetectionOutcome::Detected { codec, .. } = catalog.detect(&ctx, root).unwrap()
    else {
        panic!("synthetic bundle has one detector")
    };
    assert_eq!(codec.id().as_str(), "onshape");
    ctx.finish_session().unwrap();
    let result = descriptor
        .codec()
        .unwrap()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.report().format(), "onshape");
    let layers = result.report().dialects().unwrap();
    assert_eq!(layers.primary().dialect().as_str(), "onshape:bundle-1");
    assert!(layers
        .iter()
        .any(|m| m.format() == "step" && m.instance() == Some("export.step")));
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(result.ir().model.assets.len(), 4);
}
