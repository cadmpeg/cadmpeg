// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` graph transfer unit tests.

#![allow(clippy::doc_markdown)]

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
pub(crate) fn recovers_techdraw_page_template_and_view_graph() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawPage" name="Page" id="2"/>
 <Object type="TechDraw::DrawSVGTemplate" name="Template" id="3"/>
 <Object type="TechDraw::DrawViewPart" name="View" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="Page"><Properties Count="2">
  <Property name="Template" type="App::PropertyLink"><Link value="Template"/></Property>
  <Property name="Views" type="App::PropertyLinkList"><LinkList count="1"><Link value="View"/></LinkList></Property>
 </Properties></Object>
 <Object name="Template"><Properties Count="1"><Property name="Template" type="App::PropertyFileIncluded"><FileIncluded file="page.svg"/></Property></Properties></Object>
 <Object name="View"><Properties Count="5">
  <Property name="Source" type="App::PropertyLink"><Link value="Model"/></Property>
  <Property name="X" type="App::PropertyDistance"><Float value="25"/></Property>
  <Property name="Y" type="App::PropertyDistance"><Float value="40"/></Property>
  <Property name="Scale" type="App::PropertyFloatConstraint"><Float value="2"/></Property>
  <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("page.svg", b"<svg/>"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("TechDraw");
    let drawings = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::DrawingRecord>("drawings")
        .expect("drawings");
    assert_eq!(drawings.len(), 3);
    let page = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Page"))
        .expect("page");
    let template = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Template"))
        .expect("template");
    let view = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("view");
    assert_eq!(
        page.template.as_deref(),
        Some("fcstd:native:object#Template")
    );
    assert_eq!(page.views, ["fcstd:native:object#View"]);
    assert_eq!(template.side_entries, ["page.svg"]);
    assert_eq!(
        view.sources[0].object.as_deref(),
        Some("fcstd:native:object#Model")
    );
    assert!(view.parameters.contains_key("Direction"));
    assert_eq!(result.ir().model.drawings.len(), 3);
    let neutral_page = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Page"))
        .expect("neutral page");
    let neutral_template = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Template"))
        .expect("neutral template");
    let neutral_view = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view");
    assert_eq!(neutral_page.kind, cadmpeg_ir::drawings::DrawingKind::Page);
    assert_eq!(
        neutral_page.template.as_deref(),
        Some(neutral_template.id.0.as_str())
    );
    assert_eq!(
        neutral_page.relationships["Views"][0].target.as_deref(),
        Some(neutral_view.id.0.as_str())
    );
    assert_eq!(neutral_template.assets.len(), 1);
    assert_eq!(neutral_view.position, Some([25.0, 40.0]));
    assert_eq!(neutral_view.scale, Some(2.0));
    assert_eq!(neutral_view.direction, Some([0.0, 0.0, 1.0]));
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    corrupted
        .model
        .drawings
        .iter_mut()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view")
        .scale = Some(0.0);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid drawing reference, order, or numeric state"));
}
