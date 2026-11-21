// SPDX-License-Identifier: Apache-2.0
//! Public decode behavior and parent-context composition.
use crate::test_support::{codec, fixture};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, InspectOptions};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, DecodeOptions};
use std::io::Cursor;

#[test]
fn synthetic_bundles_pass_neutral_and_source_fidelity_validation() {
    for name in [
        "plate-fillet",
        "revolve",
        "hole",
        "hole-forms",
        "unsupported",
        "default-plane-regions",
    ] {
        let decoded = codec()
            .decode(&mut Cursor::new(fixture(name)), &DecodeOptions::default())
            .unwrap();
        let (ir, report, fidelity) = decoded.into_parts();
        let validation =
            cadmpeg_ir::validate_neutral_with_source_fidelity(&ir, &fidelity, report.losses)
                .unwrap();
        assert!(validation.is_ok(), "{name}: {validation:#?}");
    }
}

#[test]
fn public_pipeline_retains_member_bytes_and_nested_identity() {
    let bytes = fixture("revolve");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let whole = cadmpeg_core::decode::WholeInput::of_source(root, &mut Cursor::new(&bytes))
        .unwrap()
        .unwrap();
    assert_eq!(
        codec().detect_whole(&ctx, whole).unwrap(),
        cadmpeg_ir::codec::Confidence::High
    );
    ctx.finish_session().unwrap();
    let summary = codec()
        .inspect(&mut Cursor::new(&bytes), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format(), "onshape");
    assert_eq!(summary.entries.len(), 5);
    let result = codec()
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().ir_version(), "11");
    assert_eq!(result.report().format(), "onshape");
    assert_eq!(result.ir().model.assets.len(), 5);
    assert!(result
        .report()
        .dialects()
        .unwrap()
        .iter()
        .any(|m| m.format() == "step"));
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut member = zip.by_name("partstudios/studio/export.step").unwrap();
    let mut step = Vec::new();
    std::io::Read::read_to_end(&mut member, &mut step).unwrap();
    let standalone = cadmpeg_codec_step::StepCodec::default()
        .decode(&mut Cursor::new(step), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().geometry, standalone.ir().geometry);
    for asset in &result.ir().model.assets {
        let cadmpeg_ir::assets::AssetContent::Embedded { data } = &asset.content else {
            panic!("bundle asset is embedded")
        };
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut member = zip.by_name(asset.name.as_ref().unwrap().as_str()).unwrap();
        let mut expected = Vec::new();
        std::io::Read::read_to_end(&mut member, &mut expected).unwrap();
        assert_eq!(data.as_bytes(), expected);
    }
    let (ir, _, fidelity) = result.into_parts();
    let entity = &ir.model.sketch_entities[0];
    let exactness = fidelity
        .annotations
        .exactness()
        .get(entity.id().as_str())
        .unwrap();
    assert_eq!(exactness.entity(), cadmpeg_ir::Exactness::ByteExact);
    assert_eq!(
        exactness.fields().get("geometry"),
        Some(&cadmpeg_ir::Exactness::Derived)
    );
}

#[test]
fn step_callback_keeps_the_parent_retention_budget_and_member_space() {
    let bytes = fixture("revolve");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    thread_local! { static PARENT: std::cell::Cell<*const ()> = const { std::cell::Cell::new(std::ptr::null()) }; }
    PARENT.with(|slot| slot.set(std::ptr::from_ref(&ctx).cast()));
    let backend = crate::OnshapeCodec::new(|ctx, member| {
        assert_ne!(member.location().space, cadmpeg_core::decode::SpaceId::ROOT);
        PARENT.with(|slot| assert_eq!(slot.get(), std::ptr::from_ref(ctx).cast()));
        ctx.retain_bytes(
            ctx.retained_bytes_available().checked_add(1).unwrap(),
            "nested STEP budget proof",
        )?;
        unreachable!("the parent retention budget is exhausted")
    });
    assert!(matches!(
        backend.decode_impl(&ctx, root),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn container_depth_keeps_design_and_geometry_arenas_empty() {
    let bytes = fixture("revolve");
    let options = DecodeOptions {
        depth: DecodeDepth::Container,
        ..Default::default()
    };
    let result = codec().decode(&mut Cursor::new(bytes), &options).unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
    assert!(result.ir().model.features.is_empty());
}

#[test]
fn empty_member_is_named_by_a_loss_and_preserves_the_decoded_model() {
    use std::io::{Read, Write};
    let bytes = fixture("revolve");
    let mut original = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..original.len() {
        let mut member = original.by_index(index).unwrap();
        zip.start_file(
            member.name().unwrap(),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        let mut data = Vec::new();
        member.read_to_end(&mut data).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.start_file("empty.bin", zip::write::SimpleFileOptions::default())
        .unwrap();
    let result = codec()
        .decode(
            &mut Cursor::new(zip.finish().unwrap().into_inner()),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(
            |loss| loss.code.to_string() == "onshape/container.empty-member"
                && loss.message.contains("empty.bin")
        ));
}
