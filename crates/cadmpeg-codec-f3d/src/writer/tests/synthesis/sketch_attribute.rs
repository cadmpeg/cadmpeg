// SPDX-License-Identifier: Apache-2.0
//! Neutral sketch-link attributes during source-less synthesis.

use std::io::Cursor;

use cadmpeg_ir::attributes::AttributeValue;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::{
    synthetic_geometry_with_sketch_link_smbh, SketchLinkForm,
};
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

#[test]
fn neutral_sketch_link_attribute_controls_generated_link() {
    let source = f3d_with_smbh(&synthetic_geometry_with_sketch_link_smbh(
        SketchLinkForm::Tagged("113 0 1 0 2 3"),
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    let attribute = edited
        .model
        .attributes
        .iter_mut()
        .find(|attribute| {
            attribute.values.iter().any(|value| {
                matches!(value, Some(AttributeValue::String(value)) if value == "sketch_attrib_def")
            })
        })
        .unwrap();
    *attribute.values.last_mut().unwrap() = Some(AttributeValue::String("114 0 1 0 2 3".into()));
    let plan = F3dCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        f3d_native(output.ir()).sketch_curve_links[0].sketch_curve_id,
        114
    );
    assert!(output.ir().model.attributes.iter().any(|attribute| {
        matches!(attribute.values.last(), Some(Some(AttributeValue::String(value))) if value == "114 0 1 0 2 3")
    }));
}
