// SPDX-License-Identifier: Apache-2.0
//! Feature output scope round trips.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{
    write::{target::TargetRequest, EncodeInput, Encoder},
    Codec, DecodeOptions,
};

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

fn with_scope(scope: &str) -> cadmpeg_test_support::EditableDecodeResult {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        format!(
            r#"<Keywords><Feature Name="Scoped" Type="Custom" id="1" Scope="{scope}"/></Keywords>"#
        )
        .as_bytes(),
    ));
    SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap()
        .into()
}

fn rewrite(
    document: &cadmpeg_test_support::EditableDecodeResult,
) -> cadmpeg_test_support::EditableDecodeResult {
    let mut encoded = Vec::new();
    SldprtCodec
        .plan(
            EncodeInput::new(document.ir(), None),
            TargetRequest::Inherit,
        )
        .unwrap()
        .write_to(&mut encoded)
        .unwrap();
    SldprtCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap()
        .into()
}

#[test]
fn unresolved_native_output_scope_survives_synthesis() {
    let source = with_scope("MissingBody");
    assert!(source.ir().model.features[0]
        .evaluation
        .outputs()
        .is_empty());
    let written = rewrite(&source);
    assert_eq!(
        sldprt_native(written.ir()).feature_histories[0].features[0].properties["Scope"],
        "MissingBody"
    );
    assert!(written.ir().model.features[0]
        .evaluation
        .outputs()
        .is_empty());
}

#[test]
fn clearing_resolved_output_scope_removes_native_scope() {
    let base = with_scope("MissingBody");
    let body_id = base.ir().model.bodies[0].id.to_string();
    let mut source = with_scope(&body_id);
    assert_eq!(source.ir().model.features[0].evaluation.outputs().len(), 1);
    source.ir_mut().model.features[0]
        .evaluation
        .set_outputs(cadmpeg_ir::features::DistinctMembers::default());
    let written = rewrite(&source);
    assert!(
        !sldprt_native(written.ir()).feature_histories[0].features[0]
            .properties
            .contains_key("Scope")
    );
    assert!(written.ir().model.features[0]
        .evaluation
        .outputs()
        .is_empty());
}
