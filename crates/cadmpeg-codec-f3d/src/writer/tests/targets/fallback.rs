// SPDX-License-Identifier: Apache-2.0
//! An inherited source outside the catalog uses the actual synthesis default.

use super::sourced_ir;
use crate::F3dCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn off_catalog_inherit_synthesizes_and_reads_the_reported_catalog_default() {
    let ir = sourced_ir("f3d:f3z-multi-document");
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("§5.0 selects catalog-default synthesis");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let report = serde_json::to_value(plan.report()).expect("report");
    assert_eq!(report["identity"]["target"], "f3d:manifest-3-2-0-0");
    let displacement: Vec<_> = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "target.source-dialect-displaced")
        .collect();
    assert_eq!(displacement.len(), 1);
    assert!(displacement[0].message.contains("f3d:f3z-multi-document"));
    assert!(displacement[0].message.contains("f3d:manifest-3-2-0-0"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("write the selected target");
    let decoded = F3dCodec
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
        "f3d:manifest-3-2-0-0"
    );
}

#[test]
fn off_catalog_source_without_an_image_reports_the_synthesis_target() {
    let ir = sourced_ir("f3d:f3z-multi-document");
    let fidelity = cadmpeg_ir::SourceFidelity::with_annotations(
        cadmpeg_ir::Annotations::default(),
        cadmpeg_ir::hash::digest::Sha256Digest::digest(b"missing image"),
    );
    let plan = F3dCodec
        .plan(
            EncodeInput::new(&ir, Some(&fidelity)),
            TargetRequest::Inherit,
        )
        .expect("the catalog default writes when the retained image is missing");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let report = serde_json::to_value(plan.report()).unwrap();
    assert_eq!(report["identity"]["target"], "f3d:manifest-3-2-0-0");
    let loss = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "target.source-dialect-displaced")
        .expect("the source displacement is reported");
    assert!(loss.message.contains("f3d:f3z-multi-document"));
    assert!(loss.message.contains("f3d:manifest-3-2-0-0"));
}
