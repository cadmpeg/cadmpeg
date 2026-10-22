// SPDX-License-Identifier: Apache-2.0
//! An inherited source outside the catalog uses the actual synthesis default.

use super::point_file_at_version_flag;
use crate::IgesCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn off_catalog_inherit_synthesizes_and_reads_the_reported_catalog_default() {
    let source = point_file_at_version_flag(1);
    let decoded = IgesCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("original version-1 point fixture");
    let ir = decoded.ir();
    let plan = IgesCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("§5.0 selects catalog-default synthesis");
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    let report = serde_json::to_value(plan.report()).expect("report");
    assert_eq!(report["identity"]["target"], "iges:5.3-fixed-ascii");
    let displacement: Vec<_> = plan
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "target.source-dialect-displaced")
        .collect();
    assert_eq!(displacement.len(), 1);
    assert!(displacement[0].message.contains("iges:1.0-fixed-ascii"));
    assert!(displacement[0].message.contains("iges:5.3-fixed-ascii"));
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes)
        .expect("write the selected target");
    let decoded = IgesCodec
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
        "iges:5.3-fixed-ascii"
    );
}
