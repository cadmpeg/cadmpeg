// SPDX-License-Identifier: Apache-2.0
//! `Part::Extrusion` direction unit tests.

use crate::design::tests::extrusion_definition;
use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::features::extents::ExtrusionDirectionSource;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn a_profile_normal_part_extrusion_on_an_unresolved_sketch_frame_keeps_its_direction() {
    let decode = |dir: &str, reversed: bool| {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Extrusion" name="Extrusion" id="2"/>
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
</Objects>
<ObjectData Count="2">
 <Object name="Profile"><Properties Count="1">
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="NaN"/></Property>
 </Properties></Object>
 <Object name="Extrusion"><Properties Count="{}">
  <Property name="Base" type="App::PropertyLink"><Link value="Profile"/></Property>{dir}
  <Property name="DirMode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="LengthFwd" type="App::PropertyDistance"><Float value="5"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="{reversed}"/></Property>
 </Properties></Object>
</ObjectData></Document>"#,
            if dir.is_empty() { 4 } else { 5 }
        );
        FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .expect("an extrusion on an unresolved sketch frame")
    };
    let direction = |result: &cadmpeg_ir::codec::DecodeResult| match extrusion_definition(result) {
        FeatureDefinition::Operation(FeatureOperation::Extrude { direction, .. }) => {
            direction.clone()
        }
        other => panic!("{other:?}"),
    };
    let stored = r#"
  <Property name="Dir" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="7"/></Property>"#;
    for (dir, reversed, expected) in [
        (
            stored,
            false,
            cadmpeg_ir::features::extents::ExtrudeDirection::Explicit {
                vector: cadmpeg_ir::features::geometry::FeatureDirection3::from(
                    cadmpeg_ir::units::UnitVector3::Z_AXIS,
                ),
                source: Some(ExtrusionDirectionSource::ProfileNormal {}),
            },
        ),
        (
            "",
            false,
            cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {},
        ),
        (
            "",
            true,
            cadmpeg_ir::features::extents::ExtrudeDirection::ReversedProfileNormal {},
        ),
    ] {
        let result = decode(dir, reversed);
        assert_eq!(direction(&result), expected, "{dir} {reversed}");
        assert_valid_document(result.ir());
    }
}
