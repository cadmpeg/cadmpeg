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
