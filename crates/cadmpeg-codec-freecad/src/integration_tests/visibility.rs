// SPDX-License-Identifier: Apache-2.0
//! Structural shape roles take precedence over saved GUI visibility.

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::fmt::Write;
use std::io::Cursor;

use crate::test_support::test_archive::{archive_entries, assert_valid_document};
use crate::FcstdCodec;

#[test]
fn visible_body_and_tip_display_only_the_body_shape() {
    assert_visibility(true, true, false);
}

#[test]
fn hidden_tip_keeps_the_body_shape_visible() {
    assert_visibility(true, false, false);
}

#[test]
fn free_feature_keeps_its_gui_visibility() {
    assert_visibility(true, true, true);
}

#[test]
fn hidden_body_is_not_replaced_by_its_visible_tip() {
    assert_visibility(false, true, false);
}

fn assert_visibility(body_visible: bool, tip_visible: bool, free_visible: bool) {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="PartDesign::Body" name="Body"/>
 <Object type="Sketcher::SketchObject" name="Sketch"/>
 <Object type="PartDesign::Pad" name="Pad"/>
 <Object type="Part::Feature" name="Free"/>
</Objects>
<ObjectData Count="4">
 <Object name="Body"><Properties Count="3">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="Sketch"/><Link value="Pad"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="Pad"/></Property>
  <Property name="Shape" type="Part::PropertyPartShape"><Part file="Body.Shape.brp"/></Property>
 </Properties></Object>
 <Object name="Sketch"><Properties Count="1">
  <Property name="Shape" type="Part::PropertyPartShape"><Part file="Sketch.Shape.brp"/></Property>
 </Properties></Object>
 <Object name="Pad"><Properties Count="2">
  <Property name="Shape" type="Part::PropertyPartShape"><Part file="Pad.Shape.brp"/></Property>
  <Property name="AddSubShape" type="Part::PropertyPartShape"><Part file="Pad.AddSubShape.brp"/></Property>
 </Properties></Object>
 <Object name="Free"><Properties Count="1">
  <Property name="Shape" type="Part::PropertyPartShape"><Part file="Free.Shape.brp"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let mut gui = String::from(r#"<Document SchemaVersion="1"><ViewProviderData Count="4">"#);
    for (name, visible) in [
        ("Body", body_visible),
        ("Sketch", false),
        ("Pad", tip_visible),
        ("Free", free_visible),
    ] {
        write!(
            gui,
            r#"<ViewProvider name="{name}"><Properties Count="1"><Property name="Visibility" type="App::PropertyBool"><Bool value="{visible}"/></Property></Properties></ViewProvider>"#
        )
        .expect("GUI provider XML");
    }
    gui.push_str(r#"</ViewProviderData><Camera settings=""/></Document>"#);
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 1
Ve 0.001 0 0 0 0 0 1001000 *
+1 0 *";
    let mut entries = vec![
        ("Document.xml", &document[..]),
        ("GuiDocument.xml", gui.as_bytes()),
    ];
    for name in [
        "Body.Shape.brp",
        "Sketch.Shape.brp",
        "Pad.Shape.brp",
        "Pad.AddSubShape.brp",
        "Free.Shape.brp",
    ] {
        entries.push((name, &brep[..]));
    }
    let bytes = archive_entries(&entries);
    for depth in [DecodeDepth::Geometry, DecodeDepth::Full] {
        let decoded = FcstdCodec
            .decode(
                &mut Cursor::new(&bytes),
                &DecodeOptions {
                    depth,
                    ..DecodeOptions::default()
                },
            )
            .expect("body and free feature shapes with GUI visibility");
        let ir = decoded.ir();
        assert_eq!(ir.model.bodies.len(), 5);
        assert_eq!(ir.model.vertices.len(), 5);
        for (name, visible) in [
            ("Body.Shape.brp", body_visible),
            ("Sketch.Shape.brp", false),
            ("Pad.Shape.brp", false),
            ("Pad.AddSubShape.brp", false),
            ("Free.Shape.brp", free_visible),
        ] {
            let body = ir
                .model
                .bodies
                .iter()
                .find(|body| body.id.as_str().contains(&format!(":{name}:")))
                .expect("retained shape body");
            assert_eq!(body.visible, Some(visible), "{depth}: {name}");
        }
        assert_eq!(
            ir.model
                .bodies
                .iter()
                .filter(|body| body.visible != Some(false))
                .count(),
            usize::from(body_visible) + usize::from(free_visible)
        );
        assert_valid_document(ir);
        assert!(crate::test_support::validate_native(ir).is_empty());
    }
}
