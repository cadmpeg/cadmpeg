// SPDX-License-Identifier: Apache-2.0
//! Attachment-frame transfer unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn retains_support_attachment_and_distinct_offset_frame() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Feature" name="Support" id="1"/>
 <Object type="Sketcher::SketchObject" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Support"><Properties Count="0"/></Object>
 <Object name="Sketch"><Properties Count="5">
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
  <Property name="AttachmentSupport" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="Support" sub="Face1"/></LinkSubList></Property>
  <Property name="MapMode" type="App::PropertyEnumeration"><Integer value="5"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="AttachmentOffset" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("attachment");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let attachments = namespace
        .arena_as::<crate::native::AttachmentRecord>("attachments")
        .expect("attachments");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].map_mode.as_deref(), Some("5"));
    assert_eq!(
        attachments[0].supports[0].object.as_deref(),
        Some("fcstd:native:object#Support")
    );
    assert_eq!(attachments[0].supports[0].subelements, ["Face1"]);
    assert_eq!(attachments[0].placement.expect("placement")[0][3], 10.0);
    assert_eq!(attachments[0].offset.expect("offset")[0][3], 2.0);
    assert_eq!(attachments[0].effective_frame[0][3], 12.0);
    let sketch = result.ir().model.sketches.first().expect("sketch");
    assert_eq!(
        sketch
            .resolved_placement()
            .expect("resolved sketch placement")
            .0
            .x,
        10.0
    );
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn rejects_ambiguous_attachment_carriers() {
    for property in [
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1"/><PropertyPlacement Px="2"/></Property>"#,
        r#"<Property name="MapMode" type="App::PropertyString"><String value="FlatFace"/><String value="Deformed"/></Property>"#,
        r#"<Property name="AttachmentSupport" type="App::PropertyLinkSub"><LinkSub value="Support" count="1"><Sub value="Face1"/></LinkSub></Property>"#,
        r#"<Property name="MapMode" type="App::PropertyEnumeration"><Integer value="55"/></Property>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">{property}</Properties></Object></ObjectData>
</Document>"#
        );
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn rejects_noncanonical_map_mode_value_grammar() {
    for property in [
        r#"<Property name="MapMode" type="App::PropertyEnumeration"><String value="FlatFace"/></Property>"#,
        r#"<Property name="MapMode" type="App::PropertyEnumeration"><Integer value="5"/><CustomEnumList count="1"><Enum value="FlatFace"/></CustomEnumList></Property>"#,
        r#"<Property name="MapMode" type="App::PropertyEnumeration"><Integer/></Property>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">{property}</Properties></Object></ObjectData>
</Document>"#
        );
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn rejects_invalid_attachment_placement_values() {
    for property in [
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="NaN"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="0"/></Property>"#,
        r#"<Property name="Placement" type="App::PropertyString"/>"#,
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">{property}</Properties></Object></ObjectData>
</Document>"#
        );
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}
