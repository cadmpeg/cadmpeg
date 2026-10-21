// SPDX-License-Identifier: Apache-2.0
//! Extent-dependent draft admission and extreme finite extrusion directions.

use crate::design::tests::extrusion_definition;
use crate::test_support::test_archive::archive;
use crate::FcstdCodec;
use cadmpeg_ir::features::{
    ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureOperation, LinearTermination,
};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn a_second_side_draft_is_read_only_by_the_extent_that_carries_one() {
    let document = |side_type: u32| {
        format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Pad" name="Pad" id="3"/>
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
</Objects>
<ObjectData Count="2">
 <Object name="Profile"><Properties Count="0"/></Object>
 <Object name="Pad"><Properties Count="6">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="SideType" type="App::PropertyEnumeration"><Integer value="{side_type}"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="Type2" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="TaperAngle2" type="App::PropertyAngle"><Float value="120"/></Property>
 </Properties></Object>
</ObjectData></Document>"#
        )
    };

    let one_sided = FcstdCodec.decode(
        &mut Cursor::new(archive(&document(0))),
        &DecodeOptions::default(),
    );
    assert!(
        one_sided.is_ok(),
        "a one-sided pad states no second side to receive TaperAngle2"
    );

    let two_sided = FcstdCodec.decode(
        &mut Cursor::new(archive(&document(1))),
        &DecodeOptions::default(),
    );
    let error = two_sided
        .expect_err("a two-sided pad carries the second side's draft")
        .to_string();
    assert!(error.contains("TaperAngle2"), "{error}");
}

#[test]
fn implicit_extrusion_length_preserves_a_large_finite_direction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Part::Feature" name="Profile"/><Object type="Part::Extrusion" name="Extrusion"/></Objects>
<ObjectData Count="2"><Object name="Profile"><Properties Count="0"/></Object>
<Object name="Extrusion"><Properties Count="4">
<Property name="Base" type="App::PropertyLink"><Link value="Profile"/></Property>
<Property name="Dir" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1e200"/></Property>
<Property name="LengthFwd" type="App::PropertyDistance"><Float value="0"/></Property>
<Property name="LengthRev" type="App::PropertyDistance"><Float value="0"/></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(matches!(extrusion_definition(&result),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided { side: ExtrudeSide {
                termination: LinearTermination::Blind { length }, .. } }, ..
        }) if result.ir().dimension_value(length).expect("resolved dimension").get() == 1e200));
}
