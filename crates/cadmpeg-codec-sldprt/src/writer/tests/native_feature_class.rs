// SPDX-License-Identifier: Apache-2.0
//! Native feature classes in source-less writes.
#![allow(clippy::unwrap_used)]

use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;
use cadmpeg_ir::codec::{
    write::{target::TargetRequest, EncodeInput, Encoder},
    Codec, DecodeOptions,
};
use std::io::Cursor;

#[test]
fn classless_native_extrusion_remains_classless_when_unchanged() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12.5mm</Dimension></Extrusion></Keywords>"#));
    let source = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(sldprt_native(source.ir()).feature_histories[0].features[0]
        .input_class
        .is_none());
    let mut bytes = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(source.ir(), None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(sldprt_native(written.ir()).feature_histories[0].features[0]
        .input_class
        .is_none());
    assert!(matches!(
        written.ir().model.features[0].definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Extrude {
                op: cadmpeg_ir::features::BooleanOp::Join,
                ..
            }
        )
    ));
    assert_eq!(
        written.ir().model.parameters[0].value,
        source.ir().model.parameters[0].value
    );
}

#[test]
fn source_less_feature_without_a_name_stays_unnamed() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Extrusion Name="Boss" Type="BossExtrude" id="7"><Dimension Name="Depth">12.5mm</Dimension></Extrusion></Keywords>"#));
    let source = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut ir = source.ir().clone();
    ir.source = None;
    ir.native = cadmpeg_ir::Native::default();
    ir.model.features[0].name = None;
    ir.model.features[0].native_ref = None;
    let mut bytes = Vec::new();
    SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap()
        .write_to(&mut bytes)
        .unwrap();
    let written = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(written.ir().model.features[0].name, None);
    let native = sldprt_native(written.ir());
    assert!(native
        .feature_input_lanes
        .iter()
        .any(|lane| { lane.names.iter().any(|name| name.value.is_empty()) }));
    assert!(native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.names)
        .all(|name| !name.value.starts_with("Feature")));
}
