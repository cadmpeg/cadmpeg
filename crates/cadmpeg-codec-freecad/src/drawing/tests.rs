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
  <Property name="Scale" type="App::PropertyFloatConstraint"><Float value="2" min="0.1" max="10" step="0.1"/></Property>
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
    let crate::native::DrawingRole::Page {
        views,
        template: page_template,
    } = &page.role
    else {
        panic!("page record is not DrawingRole::Page");
    };
    assert_eq!(
        page_template.as_deref(),
        Some("fcstd:native:object#Template")
    );
    assert_eq!(views.as_slice(), ["fcstd:native:object#View"]);
    assert_eq!(template.side_entries, ["page.svg"]);
    assert_eq!(view.sources[0].object(), Some("fcstd:native:object#Model"));
    assert!(view.parameters.contains_key("Direction"));
    assert_eq!(
        view.parameters["Scale"],
        r#"<Float value="2" min="0.1" max="10" step="0.1"/>"#
    );
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
        Some(neutral_template.id.as_str())
    );
    assert_eq!(
        neutral_page.relationships["Views"][0].local_target(),
        Some(neutral_view.id.as_str())
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

#[test]
fn preserves_null_and_non_drawing_page_links_in_typed_relationships() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawPage" name="PageNull" id="2"/>
 <Object type="TechDraw::DrawPage" name="PageModel" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="PageNull"><Properties Count="2">
  <Property name="Template" type="App::PropertyLink"><Link value=""/></Property>
  <Property name="Views" type="App::PropertyLinkList"><LinkList count="1"><Link value="Model"/></LinkList></Property>
 </Properties></Object>
 <Object name="PageModel"><Properties Count="2">
  <Property name="Template" type="App::PropertyLink"><Link value="Model"/></Property>
  <Property name="Views" type="App::PropertyLinkList"><LinkList count="1"><Link value="Model"/></LinkList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("typed page links");
    let pages = result
        .ir()
        .model
        .drawings
        .iter()
        .filter(|drawing| drawing.kind == cadmpeg_ir::drawings::DrawingKind::Page)
        .collect::<Vec<_>>();
    assert_eq!(pages.len(), 2);
    let null_page = pages
        .iter()
        .find(|drawing| drawing.object.ends_with("#PageNull"))
        .expect("null page");
    assert!(null_page.template.is_none());
    assert!(null_page.relationships["Template"][0].is_null());
    assert_eq!(
        null_page.relationships["Views"][0].local_target(),
        Some("fcstd:native:object#Model")
    );
    let model_page = pages
        .iter()
        .find(|drawing| drawing.object.ends_with("#PageModel"))
        .expect("model page");
    assert!(model_page.template.is_none());
    assert_eq!(
        model_page.relationships["Template"][0].local_target(),
        Some("fcstd:native:object#Model")
    );
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn keeps_non_page_template_links_out_of_neutral_page_field() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="TechDraw::DrawPage" name="Page" id="1"/>
 <Object type="TechDraw::DrawSVGTemplate" name="Template" id="2"/>
 <Object type="TechDraw::DrawViewPart" name="View" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Page"><Properties Count="2">
  <Property name="Template" type="App::PropertyLink"><Link value="Template"/></Property>
  <Property name="Views" type="App::PropertyLinkList"><LinkList count="1"><Link value="View"/></LinkList></Property>
 </Properties></Object>
 <Object name="Template"><Properties Count="0"/></Object>
 <Object name="View"><Properties Count="1">
  <Property name="Template" type="App::PropertyLink"><Link value="Template"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("page and non-page template links");
    let native_drawings = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::DrawingRecord>("drawings")
        .expect("drawings");
    let native_view = native_drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("native view");
    assert_eq!(
        native_view.relationships["Template"][0].object(),
        Some("fcstd:native:object#Template")
    );

    let neutral_template = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Template"))
        .expect("neutral template");
    let neutral_page = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Page"))
        .expect("neutral page");
    let neutral_view = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view");
    assert_eq!(
        neutral_page.template.as_deref(),
        Some(neutral_template.id.as_str())
    );
    assert_eq!(
        neutral_view.relationships["Template"][0].local_target(),
        Some(neutral_template.id.as_str())
    );
    assert!(neutral_view.template.is_none());
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn accepts_enumeration_metadata_and_registered_optional_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1">
 <Object type="TechDraw::DrawViewPart" name="View" id="1"/>
</Objects>
<ObjectData Count="1">
 <Object name="View"><Properties Count="13">
  <Property name="X" type="App::PropertyDistance"><Float value="25"/></Property>
  <Property name="Y" type="App::PropertyDistance"><Float value="40"/></Property>
  <Property name="Scale" type="App::PropertyFloatConstraint"><Float value="2"/></Property>
  <Property name="ScaleType" type="App::PropertyEnumeration"><Integer value="1"/><CustomEnumList count="2"><Enum value="Page"/><Enum value="Custom"/></CustomEnumList></Property>
  <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
  <Property name="XDirection" type="App::PropertyVector"><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property>
  <Property name="Rotation" type="App::PropertyAngle"><Float value="15"/></Property>
  <Property name="Caption" type="App::PropertyString"><String value="VIEW"/></Property>
  <Property name="LockPosition" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Perspective" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/><CustomEnumList count="1"><Enum value="Normal"/></CustomEnumList></Property>
  <Property name="FormatSpecOverTolerance" type="App::PropertyString"><String value="+0.1"/></Property>
  <Property name="FormatSpecUnderTolerance" type="App::PropertyString"><String value="-0.1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("typed drawing carriers");

    let drawing = &result.ir().model.drawings[0];
    assert_eq!(drawing.position, Some([25.0, 40.0]));
    assert_eq!(drawing.scale, Some(2.0));
    assert_eq!(drawing.direction, Some([0.0, 0.0, 1.0]));
    assert_eq!(drawing.rotation_degrees, Some(15.0));
    assert_eq!(drawing.parameters["ScaleType"], r#"<Integer value="1"/>"#);
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
}

#[test]
fn rejects_noncanonical_page_link_carriers() {
    for document in [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="TechDraw::DrawPage" name="Page" id="1"/>
 <Object type="TechDraw::DrawSVGTemplate" name="First" id="2"/>
 <Object type="TechDraw::DrawSVGTemplate" name="Second" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Page"><Properties Count="1">
  <Property name="Template" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Second"><Properties Count="0"/></Object>
</ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="TechDraw::DrawPage" name="Page" id="1"/>
 <Object type="TechDraw::DrawViewPart" name="View" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Page"><Properties Count="1">
  <Property name="Views" type="App::PropertyLink"><Link value="View"/></Property>
 </Properties></Object>
 <Object name="View"><Properties Count="0"/></Object>
</ObjectData></Document>"#,
    ] {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            )
            .expect_err("noncanonical page link carrier");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn rejects_duplicate_drawing_carrier_properties_and_values() {
    let documents = [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="3">
 <Property name="X" type="App::PropertyDistance"><Float value="10"/><Float value="11"/></Property>
 <Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property>
 <Property name="Scale" type="App::PropertyFloat"><Float value="1"/></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="4">
 <Property name="X" type="App::PropertyDistance"><Float value="10"/></Property>
 <Property name="X" type="App::PropertyDistance"><Float value="11"/></Property>
 <Property name="Y" type="App::PropertyDistance"><Float value="20"/></Property>
 <Property name="Scale" type="App::PropertyFloat"><Float value="1"/></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="2">
 <Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="1"/><PropertyVector valueX="1" valueY="0" valueZ="0"/></Property>
 <Property name="Scale" type="App::PropertyFloat"><Float value="1"/></Property>
</Properties></Object></ObjectData></Document>"#,
    ];
    for document in documents {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn rejects_noncanonical_drawing_scalar_attributes() {
    let documents = [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="1">
 <Property name="X" type="App::PropertyDistance"><Float Value="12.5"/></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="1">
 <Property name="Scale" type="App::PropertyFloatConstraint"><Float value="1.75" Value="2.5"/></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="1">
 <Property name="Rotation" type="App::PropertyAngle"><Float value="15.5" unit="deg"/></Property>
</Properties></Object></ObjectData></Document>"#,
    ];
    for document in documents {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn classifies_drawing_runtime_types_exactly() {
    assert_eq!(
        super::classify("TechDraw::DrawPage"),
        cadmpeg_ir::drawings::DrawingKind::Page
    );
    assert_eq!(
        super::classify("TechDraw::DrawViewSection"),
        cadmpeg_ir::drawings::DrawingKind::Section
    );
    assert_eq!(
        super::classify("TechDraw::DrawViewDimension"),
        cadmpeg_ir::drawings::DrawingKind::Dimension
    );
    assert_eq!(
        super::classify("TechDraw::VendorDrawPage"),
        cadmpeg_ir::drawings::DrawingKind::Other
    );
    assert_eq!(
        super::classify("Vendor::TechDraw::DrawPage"),
        cadmpeg_ir::drawings::DrawingKind::Other
    );
}

#[test]
fn classifies_standard_derived_drawing_types() {
    for (runtime_type, kind) in [
        (
            "TechDraw::DrawBrokenView",
            cadmpeg_ir::drawings::DrawingKind::View,
        ),
        (
            "TechDraw::DrawComplexSection",
            cadmpeg_ir::drawings::DrawingKind::Section,
        ),
        (
            "TechDraw::DrawParametricTemplate",
            cadmpeg_ir::drawings::DrawingKind::Template,
        ),
        (
            "TechDraw::DrawViewMulti",
            cadmpeg_ir::drawings::DrawingKind::View,
        ),
        (
            "TechDraw::DrawViewArch",
            cadmpeg_ir::drawings::DrawingKind::View,
        ),
        (
            "TechDraw::DrawViewDraft",
            cadmpeg_ir::drawings::DrawingKind::View,
        ),
        (
            "TechDraw::DrawViewCollection",
            cadmpeg_ir::drawings::DrawingKind::Other,
        ),
    ] {
        assert_eq!(super::classify(runtime_type), kind, "{runtime_type}");
    }
}

#[test]
fn retains_unknown_techdraw_runtime_types_only_in_native_records() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="TechDraw::VendorView" name="Unknown" id="1"/>
 <Object type="TechDraw::DrawViewArch" name="Arch" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Unknown"><Properties Count="0"/></Object>
 <Object name="Arch"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("drawing registry");
    let drawings = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::DrawingRecord>("drawings")
        .expect("drawings");
    assert_eq!(drawings.len(), 1);
    assert!(drawings[0].object.ends_with("#Arch"));
    assert_eq!(result.ir().model.drawings.len(), 1);
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_wrong_drawing_carrier_types() {
    let documents = [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="1">
<Property name="X" type="App::PropertyString"><String value="1"/></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="1">
<Property name="Source" type="App::PropertyString"><String value="Model"/></Property>
</Properties></Object></ObjectData></Document>"#,
    ];
    for document in documents {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}

#[test]
fn rejects_invalid_drawing_numeric_admission() {
    let documents = [
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="1">
<Property name="Scale" type="App::PropertyFloatConstraint"><Float value="-1"/></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="1">
<Property name="X" type="App::PropertyDistance"><Float value="nan"/></Property>
</Properties></Object></ObjectData></Document>"#,
        r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View" id="1"/></Objects>
<ObjectData Count="1"><Object name="View"><Properties Count="1">
<Property name="Direction" type="App::PropertyVector"><PropertyVector valueX="0" valueY="0" valueZ="0"/></Property>
</Properties></Object></ObjectData></Document>"#,
    ];
    for document in documents {
        assert!(matches!(
            FcstdCodec.decode(
                &mut Cursor::new(archive(document)),
                &DecodeOptions::default(),
            ),
            Err(cadmpeg_ir::DecodeFailure::Codec(
                cadmpeg_core::CodecError::Malformed(_)
            ))
        ));
    }
}
