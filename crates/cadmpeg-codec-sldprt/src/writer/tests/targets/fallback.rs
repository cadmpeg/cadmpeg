// SPDX-License-Identifier: Apache-2.0
//! An inherited source outside the catalog uses the actual synthesis default.

use super::sourced_ir;
use crate::SldprtCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

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
    let error = crate::writer::write_semantic_with_records(
        &ir,
        &fidelity.annotations,
        &records,
        Some(&fidelity),
        &mut Vec::new(),
    )
    .err()
    .expect("the actual retained container is truncated");
    assert!(matches!(
        &error,
        cadmpeg_core::CodecError::Truncated { operation, .. }
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
fn an_intact_unchanged_source_still_patches_its_exact_container() {
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
        WritePath::Patched { .. }
    ));
    assert!(plan.report().synthesis_cause().is_none());
    let mut written = Vec::new();
    let report = plan.write_to(&mut written).expect("write intact source");
    let redecoded = SldprtCodec
        .decode(&mut Cursor::new(written), &DecodeOptions::default())
        .expect("patched output decodes");
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &ir, redecoded.ir());
}
