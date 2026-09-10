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
    assert_eq!(
        attachments[0].map_mode.map(|mode| mode.to_string()),
        Some("5".to_owned())
    );
    assert_eq!(
        attachments[0].supports[0]
            .as_ref()
            .expect("support")
            .object(),
        Some("fcstd:native:object#Support")
    );
    assert_eq!(
        attachments[0].supports[0]
            .as_ref()
            .expect("support")
            .subelements(),
        ["Face1"]
    );
    assert_eq!(
        attachments[0].placement().expect("placement").rows()[0][3],
        10.0
    );
    assert_eq!(attachments[0].offset().expect("offset").rows()[0][3], 2.0);
    assert_eq!(attachments[0].effective_frame()[0][3], 12.0);
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
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
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
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
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
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn map_mode_admission_checks_indices_and_preserves_decimal_wire() {
    for index in 0..super::MAP_MODE_NAMES.len() {
        let expected = index.to_string();
        let mode = super::MapModeIndex::try_new(index).expect("valid table index");
        assert_eq!(mode.to_string(), expected);
        assert_eq!(
            super::MapModeIndex::try_from(expected.as_str()).expect("text index"),
            mode
        );
        let wire = serde_json::json!(expected);
        assert_eq!(serde_json::to_value(mode).expect("serialize index"), wire);
        assert_eq!(
            serde_json::from_value::<super::MapModeIndex>(wire).expect("deserialize index"),
            mode
        );
    }
    for index in [super::MAP_MODE_NAMES.len(), 255, 256, usize::MAX] {
        assert!(super::MapModeIndex::try_new(index).is_err());
    }
    let record = crate::native::AttachmentRecord::try_new(
        "attachment".to_owned(),
        "object".to_owned(),
        Vec::new(),
        None,
        None,
        None,
    )
    .expect("attachment without a map mode");
    let wire = serde_json::to_value(record).expect("serialize attachment");
    for value in [
        "not-an-index".to_owned(),
        "-1".to_owned(),
        String::new(),
        super::MAP_MODE_NAMES.len().to_string(),
        "256".to_owned(),
    ] {
        assert!(super::MapModeIndex::try_from(value.as_str()).is_err());
        let mut invalid = wire.clone();
        invalid["map_mode"] = serde_json::json!(value);
        let error = serde_json::from_value::<crate::native::AttachmentRecord>(invalid)
            .expect_err("invalid map mode at the record wire boundary");
        assert!(error.to_string().contains("map_mode"));
    }
}
