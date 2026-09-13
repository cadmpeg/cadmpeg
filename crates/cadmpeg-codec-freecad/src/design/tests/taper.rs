// SPDX-License-Identifier: Apache-2.0
//! Extrude draft-angle transfer unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::features::{ExtrudeExtent, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;
#[test]
fn a_zero_taper_angle_is_the_native_no_draft_sentinel() {
    let pad = |taper: &str| {
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Pad" name="Pad" id="2"/>
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
</Objects>
<ObjectData Count="2">
 <Object name="Profile"><Properties Count="0"/></Object>
 <Object name="Pad"><Properties Count="4">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="TaperAngle" type="App::PropertyAngle"><Float value="{taper}"/></Property>
 </Properties></Object>
</ObjectData></Document>"#
        )
    };
    let draft_of = |result: &cadmpeg_ir::codec::DecodeResult| {
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided { side },
            ..
        }) = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Pad"))
            .expect("pad feature")
            .evaluation
            .definition()
        else {
            unreachable!("a blind pad decodes as a one-sided extrude")
        };
        side.draft
    };

    let zero = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&pad("0"))),
            &DecodeOptions::default(),
        )
        .expect("a zero taper decodes");
    assert!(draft_of(&zero).is_none());

    let five = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&pad("5"))),
            &DecodeOptions::default(),
        )
        .expect("a five-degree taper decodes");
    assert!(draft_of(&five).is_some());

    // A taper SlopeAngle refuses is reported, not swallowed with the feature.
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&pad("90"))),
            &DecodeOptions::default(),
        )
        .expect_err("a right-angle taper is no slope angle");
    assert!(error.to_string().contains("TaperAngle"), "{error}");
}
