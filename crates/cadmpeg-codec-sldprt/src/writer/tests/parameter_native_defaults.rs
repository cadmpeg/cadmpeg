// SPDX-License-Identifier: Apache-2.0
//! Preserve optional native dimension attributes.
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
fn absent_default_dimension_attributes_stay_absent() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x42, "Contents/Keywords", br#"<Keywords><Feature Name="Owner" Type="Custom" id="1"><Dimension Name="Depth">12.5mm</Dimension></Feature></Keywords>"#));
    let source = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(sldprt_native(source.ir()).feature_histories[0].features[0]
        .dimension_properties
        .is_empty());
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
        .dimension_properties
        .is_empty());
    assert_eq!(
        written.ir().model.parameters[0].value,
        source.ir().model.parameters[0].value
    );
    assert_eq!(
        written.ir().model.parameters[0].driving,
        source.ir().model.parameters[0].driving
    );
}
