// SPDX-License-Identifier: Apache-2.0
//! An inherited source outside the catalog uses the actual synthesis default.

use super::sourced_ir;
use crate::SldprtCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn semantic_source_scan_shares_the_caller_input_account() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};

    let ir = sourced_ir("sldprt:unknown");
    let mut fidelity = cadmpeg_ir::SourceFidelity::with_annotations(
        cadmpeg_ir::Annotations::default(),
        cadmpeg_ir::hash::digest::Sha256Digest::digest(b"\x42"),
    );
    let image = cadmpeg_ir::RetainedSourceRecord::from_bytes(
        "file",
        0,
        cadmpeg_ir::source_fidelity::RetainedBytes::Inline { data: vec![0x42] },
    )
    .unwrap();
    fidelity
        .insert_retained_record(crate::source_image_id(), image)
        .unwrap();
    let records = crate::source_records(&ir, &fidelity);
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_input_bytes: 1,
            ..Default::default()
        },
        ..DecodePolicy::default()
    };
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = cadmpeg_core::encode::EncodeSession::from_context(&ctx).unwrap();
    let prior_arena = DecodeArena::new();
    let (_prior, _) = DecodeContext::for_encode_session_root(&prior_arena, &session, b"x").unwrap();
    let mut bytes = Vec::new();
    let result = crate::writer::write_semantic_at(
        &ctx,
        &ir,
        &fidelity.annotations,
        &records,
        crate::writer::SemanticOptions {
            class_layouts: crate::writer::ClassLayouts::Source,
            sw_version: None,
            prepared_native: None,
            source_trust: crate::writer::SourceTrust::Authored,
        },
        &mut bytes,
    );
    assert!(matches!(
        result,
        Err(crate::writer::write_error::WriteError::Fatal(cadmpeg_core::CodecError::ResourceLimit(limit)))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::InputBytes
                && limit.limit == 1 && limit.used == 1 && limit.additional == 1
    ));
    assert!(bytes.is_empty());
}

#[test]
fn off_catalog_inherit_synthesizes_and_reads_the_reported_catalog_default() {
    let ir = sourced_ir("sldprt:future");
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("§5.0 selects catalog-default synthesis");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let report = serde_json::to_value(plan.report()).expect("report");
    assert_eq!(report["identity"]["target"], "sldprt:unknown");
    let displacement: Vec<_> = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "target.source-dialect-displaced")
        .collect();
    assert_eq!(displacement.len(), 1);
    assert!(displacement[0].message.contains("sldprt:future"));
    assert!(displacement[0].message.contains("sldprt:unknown"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("write the selected target");
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("actual target grammar reads");
    assert_eq!(
        decoded
            .report()
            .dialects()
            .expect("native classification")
            .primary()
            .dialect()
            .as_str(),
        "sldprt:unknown"
    );
}

#[test]
fn a_truncated_retained_container_declines_only_the_patch_and_writes_current_content() {
    let ir = sourced_ir("sldprt:unknown");
    let mut fidelity = cadmpeg_ir::SourceFidelity::with_annotations(
        cadmpeg_ir::Annotations::default(),
        cadmpeg_ir::hash::digest::Sha256Digest::digest(b"\x42"),
    );
    let image = cadmpeg_ir::RetainedSourceRecord::from_bytes(
        "file",
        0,
        cadmpeg_ir::source_fidelity::RetainedBytes::Inline { data: vec![0x42] },
    )
    .expect("retained source bytes");
    fidelity
        .insert_retained_record(crate::source_image_id(), image)
        .expect("source image identity");
    let records = crate::source_records(&ir, &fidelity);
    let limits = cadmpeg_core::decode::ResourceLimits {
        max_input_bytes: 0,
        ..Default::default()
    };
    let limited = crate::writer::write_semantic_with_records(
        &ir,
        &fidelity.annotations,
        &records,
        Some(&fidelity),
        limits,
        &mut Vec::new(),
    )
    .err()
    .expect("the source image exceeds the caller's input ceiling");
    assert!(matches!(
        limited,
        crate::writer::write_error::WriteError::Fatal(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::InputBytes
                && limit.limit == 0
    ));
    let error = crate::writer::write_semantic_with_records(
        &ir,
        &fidelity.annotations,
        &records,
        Some(&fidelity),
        cadmpeg_core::decode::ResourceLimits::default(),
        &mut Vec::new(),
    )
    .err()
    .expect("the actual retained container is truncated");
    assert!(matches!(
        &error,
        crate::writer::write_error::WriteError::Fatal(cadmpeg_core::CodecError::Truncated { operation, .. })
            if *operation == "read SLDPRT native outer header"
    ));
    let expected_reason = error.to_string();
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("a retained-layout parse failure selects synthesis");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert!(matches!(
        plan.report().synthesis_cause(),
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason })
            if reason == &expected_reason
    ));
    let mut written = Vec::new();
    plan.write_to(&mut written).expect("write current content");
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("synthesized native container reads");
    crate::test_support::assert_source_free_synthesis_agrees(&ir, decoded.ir());
    assert_eq!(decoded.ir().model.bodies.len(), ir.model.bodies.len());
    assert_eq!(decoded.ir().model.faces.len(), ir.model.faces.len());
}

#[test]
fn an_intact_unchanged_source_still_replays_its_exact_container() {
    let mut source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="13100"><swModel swName="part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("source native container reads");
    let (ir, _, fidelity) = decoded.into_parts();
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("the unchanged source retains replay eligibility");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::VerbatimReplay { .. }
    ));
    assert!(plan.report().synthesis_cause().is_none());
    let mut written = Vec::new();
    plan.write_to(&mut written).expect("write intact source");
    assert_eq!(written, source);
}

#[test]
fn an_edited_document_does_not_replay_its_source_image() {
    let mut source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="13100"><swModel swName="part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("source native container reads");
    let (mut ir, _, fidelity) = decoded.into_parts();
    let point = ir
        .model
        .points
        .first_mut()
        .expect("the triangle body decodes points");
    point.set_position(point.position().negated());
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("an edited source still plans");
    assert!(
        !matches!(plan.report().write_path(), WritePath::VerbatimReplay { .. }),
        "an edited document must not replay its source image"
    );
}

#[test]
fn an_altered_source_image_does_not_replay() {
    let mut source = crate::test_support::history::sldprt_with_body_and_history(
        &crate::test_support::parasolid::triangle_body(),
    );
    source.extend(crate::test_support::container::make_block(
        0x43,
        "Contents/SolidWorks",
        br#"<?xml version="1.0"?><swSolidWorks swVersion="13100"><swModel swName="part" swConfigurationName="Default"/></swSolidWorks>"#,
    ));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("source native container reads");
    let (ir, _, mut fidelity) = decoded.into_parts();
    let record = fidelity
        .remove_retained_record(crate::SOURCE_IMAGE_ID)
        .expect("the decode retains its source image");
    let mut image = record
        .data()
        .expect("the retained image carries bytes")
        .to_vec();
    let middle = image.len() / 2;
    image[middle] ^= 0x01;
    let untyped = record.untyped().iter().copied().collect::<Vec<_>>();
    fidelity
        .insert_retained_record(
            crate::source_image_id(),
            cadmpeg_ir::RetainedSourceRecord::from_bytes(
                record.stream(),
                record.offset(),
                cadmpeg_ir::source_fidelity::RetainedBytes::Inline { data: image },
            )
            .expect("altered source image extent"),
        )
        .expect("source image identity is unique");
    fidelity
        .mark_untyped(crate::SOURCE_IMAGE_ID, untyped)
        .expect("the altered image keeps its untyped classes");
    let plan = SldprtCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("an altered image still plans");
    assert!(
        !matches!(plan.report().write_path(), WritePath::VerbatimReplay { .. }),
        "an altered source image must not replay"
    );
}
