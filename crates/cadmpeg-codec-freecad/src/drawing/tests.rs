// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` graph transfer unit tests.

#![allow(clippy::doc_markdown)]

use cadmpeg_test_support::wire;

use crate::test_support::test_archive::{archive, archive_entries, assert_valid_document};
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn drawing_direction_vector_refuses_nonfinite_components() {
    let mut attributes = std::collections::BTreeMap::from([
        ("valueX".into(), "0".into()),
        ("valueY".into(), "1".into()),
        ("valueZ".into(), "0".into()),
    ]);
    let value = |attributes| crate::native::ValueRecord {
        parent: None,
        tag: "PropertyVector".into(),
        order: 0,
        attributes,
        decoded_json: None,
        text: None,
    };
    assert_eq!(
        super::vector_value(&value(attributes.clone())),
        Some(cadmpeg_ir::units::FiniteVector::new([0.0, 1.0, 0.0]).unwrap())
    );
    attributes.insert("valueY".into(), "inf".into());
    assert_eq!(super::vector_value(&value(attributes)), None);
}

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
    let crate::native::TechDrawKind::Page {
        views,
        template: page_template,
        ..
    } = &page.kind
    else {
        panic!("page record is not TechDrawKind::Page");
    };
    assert_eq!(
        page_template.as_deref(),
        Some("fcstd:native:object#Template")
    );
    assert_eq!(views.as_slice(), ["fcstd:native:object#View"]);
    assert_eq!(template.side_entries, ["page.svg"]);
    assert_eq!(
        view.sources[0].as_ref().expect("source").object(),
        Some("fcstd:native:object#Model")
    );
    assert!(view.parameters.contains_key("Direction"));
    assert_eq!(view.parameters["Scale"], "2.0");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let scale = properties
        .iter()
        .find(|property| property.owner.ends_with("#View") && property.name == "Scale")
        .expect("native scale");
    assert_eq!(
        scale.values()[0].attributes,
        std::collections::BTreeMap::from([
            ("value".into(), "2".into()),
            ("min".into(), "0.1".into()),
            ("max".into(), "10".into()),
            ("step".into(), "0.1".into()),
        ])
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
        neutral_page
            .template
            .as_ref()
            .map(cadmpeg_ir::drawings::DrawingId::as_str),
        Some(neutral_template.id.as_str())
    );
    assert_eq!(
        neutral_page.relationships["Views"][0].local_target(),
        Some(neutral_view.id.as_str())
    );
    assert_eq!(neutral_template.assets.len(), 1);
    assert_eq!(
        neutral_view
            .position
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([25.0, 40.0])
    );
    assert_eq!(
        neutral_view
            .scale
            .map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(2.0)
    );
    assert_eq!(
        neutral_view
            .direction
            .map(|value| wire::value::<[f64; 3]>(&value)),
        Some([0.0, 0.0, 1.0])
    );
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());
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
    assert!(matches!(
        null_page.relationships["Template"][0].target,
        cadmpeg_ir::references::ReferenceTarget::Null
    ));
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
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
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
        native_view.relationships["Template"][0]
            .as_ref()
            .expect("relationship")
            .object(),
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
        neutral_page
            .template
            .as_ref()
            .map(cadmpeg_ir::drawings::DrawingId::as_str),
        Some(neutral_template.id.as_str())
    );
    assert_eq!(
        neutral_view.relationships["Template"][0].local_target(),
        Some(neutral_template.id.as_str())
    );
    assert!(neutral_view.template.is_none());
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
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
    assert_eq!(
        drawing.position.map(cadmpeg_ir::units::FiniteVector::get),
        Some([25.0, 40.0])
    );
    assert_eq!(
        drawing.scale.map(cadmpeg_ir::scalar::PositiveReal::get),
        Some(2.0)
    );
    assert_eq!(
        drawing
            .direction
            .map(|value| wire::value::<[f64; 3]>(&value)),
        Some([0.0, 0.0, 1.0])
    );
    assert_eq!(
        drawing
            .rotation_degrees
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        Some(15.0)
    );
    assert_eq!(drawing.parameters["ScaleType"], "1");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let scale_type = properties
        .iter()
        .find(|property| property.name == "ScaleType")
        .expect("native scale type");
    assert_eq!(
        scale_type
            .values()
            .iter()
            .filter(|value| value.tag == "Enum")
            .map(|value| value.attributes["value"].as_str())
            .collect::<Vec<_>>(),
        ["Page", "Custom"]
    );
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
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
    assert!(crate::test_support::validate_native(result.ir()).is_empty());
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

#[test]
fn decodes_python_page_kind_with_views_and_template() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
<Object type="TechDraw::DrawPagePython" name="Page"/>
<Object type="TechDraw::DrawView" name="View"/>
<Object type="TechDraw::DrawTemplate" name="Template"/>
</Objects>
<ObjectData Count="3">
<Object name="Page"><Properties Count="2">
<Property name="Views" type="App::PropertyLinkList"><LinkList count="1"><Link value="View"/></LinkList></Property>
<Property name="Template" type="App::PropertyLink"><Link value="Template"/></Property>
</Properties></Object>
<Object name="View"><Properties Count="0"/></Object>
<Object name="Template"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("decode Python page archive");
    let drawings = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace")
        .arena_as::<crate::native::DrawingRecord>("drawings")
        .expect("drawing records");
    let page = drawings
        .iter()
        .find(|record| record.object == "fcstd:native:object#Page")
        .expect("Python page");
    assert_eq!(
        page.kind,
        crate::native::TechDrawKind::Page {
            runtime: crate::native::TechDrawPageKind::Python,
            views: vec!["fcstd:native:object#View".to_owned()],
            template: Some("fcstd:native:object#Template".to_owned()),
        }
    );
}

#[test]
fn drawing_wire_rejects_non_page_views() {
    let wire = serde_json::json!({
        "id": "view", "object": "view", "kind": "TechDraw::DrawView",
        "views": ["a"], "template": null, "sources": [],
        "relationships": {}, "parameters": {}, "side_entries": []
    });
    let error = serde_json::from_value::<crate::native::DrawingRecord>(wire)
        .expect_err("non-page payload must reject page views");
    assert!(error
        .to_string()
        .contains("non-page drawing record cannot carry views or a template"));
}

#[test]
fn techdraw_sheet_units_title_block_and_dimension_fields() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="2"><Object type="TechDraw::DrawSVGTemplate" name="Template"/><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects><ObjectData Count="2"><Object name="Template"><Properties Count="2"><Property name="PageResult" type="App::PropertyFileIncluded"><FileIncluded file="sheet.svg"/></Property><Property name="EditableTexts" type="App::PropertyMap"><Map count="2"><Item key="Author" value="Test"/><Item key="Title" value="Part"/></Map></Property></Properties></Object><Object name="Dimension"><Properties Count="6"><Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property><Property name="Arbitrary" type="App::PropertyBool"><Bool value="true"/></Property><Property name="FormatSpec" type="App::PropertyString"><String value="CHECK"/></Property><Property name="OverTolerance" type="App::PropertyQuantityConstraint"><Float value="0.2"/></Property><Property name="UnderTolerance" type="App::PropertyQuantityConstraint"><Float value="-0.1"/></Property><Property name="ArbitraryTolerances" type="App::PropertyBool"><Bool value="false"/></Property></Properties></Object></ObjectData></Document>"#;
    for (width, height, expected) in [
        ("21cm", "297mm", Some((210.0, 297.0))),
        ("2in", "72pt", Some((50.8, 25.4))),
        ("6pc", "96px", Some((25.4, 25.4))),
        ("210", "297mm", None),
        ("12qu", "297mm", None),
        ("0mm", "297mm", None),
    ] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"/>"#
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("sheet.svg", svg.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();
        let sheet = result
            .ir()
            .model
            .drawings
            .iter()
            .find(|drawing| drawing.kind == cadmpeg_ir::drawings::DrawingKind::Template)
            .expect("template")
            .sheet
            .as_ref();
        assert_eq!(
            sheet.map(|s| (s.width_mm.get(), s.height_mm.get())),
            expected
        );
        if let Some(sheet) = sheet {
            assert_eq!(
                sheet
                    .fields
                    .iter()
                    .map(|f| (f.name.as_str(), f.value.as_str()))
                    .collect::<Vec<_>>(),
                [("Author", "Test"), ("Title", "Part")]
            );
        } else {
            assert!(result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code.local_code() == "drawing.sheet-not-transferred"));
        }
        let annotation = &result.ir().model.semantic_annotations[0];
        assert_eq!(
            annotation.dimension_kind,
            Some(cadmpeg_ir::semantic_annotations::DimensionKind::Distance {})
        );
        assert_eq!(annotation.text_override.as_deref(), Some("CHECK"));
        assert_eq!(
            annotation
                .upper_tolerance
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            Some(0.2)
        );
        assert_eq!(
            annotation
                .lower_tolerance
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            Some(-0.1)
        );
    }
}

#[test]
fn techdraw_persisted_curves_and_native_losses() {
    let edge = r##"<CosmeticEdge type="TechDraw::CosmeticEdge"><Style value="1"/><Weight value="0.25"/><Color value="#000000"/><Visible value="0"/><GeometryType value="7"/><GeomType value="7"/><ExtractType value="0"/><EdgeClass value="5"/><HLRVisible value="0"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="1"/><Source value="1"/><SourceIndex value="-1"/><CosmeticTag value="test"/><Points PointsCount="2"><Point X="1" Y="2" Z="0"/><Point X="3" Y="4" Z="0"/></Points></CosmeticEdge>"##;
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View"/></Objects><ObjectData Count="1"><Object name="View"><Properties Count="1"><Property name="CosmeticEdges" type="TechDraw::PropertyCosmeticEdgeList"><CosmeticEdgeList count="1">{edge}</CosmeticEdgeList></Property></Properties></Object></ObjectData></Document>"#
    );
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let curves = &result.ir().model.drawings[0].curves;
    assert_eq!(curves.len(), 1);
    assert_eq!(
        curves[0].source,
        cadmpeg_ir::drawings::CurveSource::Cosmetic
    );
    assert_eq!(
        curves[0].visibility,
        cadmpeg_ir::drawings::CurveVisibility::Hidden
    );
    assert_eq!(curves[0].displayed, Some(false));
    assert_eq!(
        curves[0].geometry,
        cadmpeg_ir::drawings::DrawingCurveGeometry::Line {
            start: cadmpeg_ir::units::FiniteVector::new([1.0, 2.0]).unwrap(),
            end: cadmpeg_ir::units::FiniteVector::new([3.0, 4.0]).unwrap(),
        }
    );
    let unsupported = document.replace("Z=\"0\"", "Z=\"1\"");
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&unsupported)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.drawings[0].curves.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "drawing.cosmetic-edge-not-transferred"));
}

#[test]
fn techdraw_unknown_dimension_and_saved_shape_stay_native() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects><ObjectData Count="1"><Object name="Dimension"><Properties Count="2"><Property name="Type" type="App::PropertyEnumeration"><Integer value="99"/></Property><Property name="SavedGeometry" type="Part::PropertyTopoShapeList"><ShapeList count="1"><TopoShape file="saved.brp"/></ShapeList></Property></Properties></Object></ObjectData></Document>"#;
    let empty = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n*";
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("saved.brp", empty),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(
        result.ir().model.semantic_annotations[0].dimension_kind,
        Some(cadmpeg_ir::semantic_annotations::DimensionKind::Other {
            native_type: "99".into()
        })
    );
    assert!(result.ir().model.semantic_annotations[0].value.is_none());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "drawing.dimension-kind-not-transferred"));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "drawing.dimension-geometry-not-transferred"));
}

#[test]
fn techdraw_dimension_custom_categories() {
    use cadmpeg_ir::semantic_annotations::DimensionKind;
    for (label, expected) in [
        ("DistanceX", DimensionKind::HorizontalDistance {}),
        ("DistanceY", DimensionKind::VerticalDistance {}),
        ("Radius", DimensionKind::Radius {}),
        ("Diameter", DimensionKind::Diameter {}),
        ("Angle", DimensionKind::Angle {}),
        ("Angle3Pt", DimensionKind::ThreePointAngle {}),
    ] {
        let document = format!(
            r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects><ObjectData Count="1"><Object name="Dimension"><Properties Count="1"><Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/><CustomEnumList count="1"><Enum value="{label}"/></CustomEnumList></Property></Properties></Object></ObjectData></Document>"#
        );
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive(&document)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(
            result.ir().model.semantic_annotations[0].dimension_kind,
            Some(expected)
        );
    }
}

#[test]
fn techdraw_circle_center_line_and_format_loss() {
    let circle = r##"<CosmeticEdge type="TechDraw::CosmeticEdge"><Style value="1"/><Weight value="0.25"/><Color value="#000000"/><Visible value="1"/><GeometryType value="1"/><GeomType value="1"/><ExtractType value="0"/><EdgeClass value="5"/><HLRVisible value="1"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="1"/><Source value="1"/><SourceIndex value="-1"/><CosmeticTag value="test"/><Center X="2" Y="3" Z="0"/><Radius value="4"/></CosmeticEdge>"##;
    let line = r##"<CenterLine type="TechDraw::CenterLine"><Start X="0" Y="1" Z="0"/><End X="0" Y="6" Z="0"/><Mode value="0"/><HShift value="0"/><VShift value="0"/><Rotate value="0"/><Extend value="0"/><Type value="1"/><Flip value="0"/><Faces FaceCount="0"/><Edges EdgeCount="0"/><CLPoints CLPointCount="0"/><Style value="1"/><Weight value="0.25"/><Color value="#000000"/><Visible value="1"/><GeometryType value="7"/><GeomType value="7"/><ExtractType value="0"/><EdgeClass value="5"/><HLRVisible value="1"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="1"/><Source value="2"/><SourceIndex value="-1"/><CosmeticTag value="test"/><Points PointsCount="2"><Point X="0" Y="1" Z="0"/><Point X="0" Y="6" Z="0"/></Points></CenterLine>"##;
    let document = format!(
        r##"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewPart" name="View"/></Objects><ObjectData Count="1"><Object name="View"><Properties Count="3"><Property name="CosmeticEdges" type="TechDraw::PropertyCosmeticEdgeList"><CosmeticEdgeList count="1">{circle}</CosmeticEdgeList></Property><Property name="CenterLines" type="TechDraw::PropertyCenterLineList"><CenterLineList count="1">{line}</CenterLineList></Property><Property name="GeomFormats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="1"><GeomFormat type="TechDraw::GeomFormat"><GeomIndex value="1"/><Style value="1"/><Weight value="0.25"/><Color value="#000000"/><Visible value="1"/></GeomFormat></GeomFormatList></Property></Properties></Object></ObjectData></Document>"##
    );
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let curves = &result.ir().model.drawings[0].curves;
    assert_eq!(curves.len(), 2);
    assert!(
        matches!(curves[0].geometry, cadmpeg_ir::drawings::DrawingCurveGeometry::Circle { radius, .. } if radius.get() == 4.0)
    );
    assert_eq!(
        curves[1].source,
        cadmpeg_ir::drawings::CurveSource::CenterLine
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "drawing.geometry-format-not-transferred"));
    let adjusted = document.replace("HShift value=\"0\"", "HShift value=\"1\"");
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(&adjusted)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().model.drawings[0].curves.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "drawing.center-line-not-transferred"));
}

#[test]
fn techdraw_nonempty_saved_geometry_has_loss_without_inferred_value() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects><ObjectData Count="1"><Object name="Dimension"><Properties Count="1"><Property name="SavedGeometry" type="Part::PropertyTopoShapeList"><ShapeList count="1"><TopoShape file="saved.brp"/></ShapeList></Property></Properties></Object></ObjectData></Document>"#;
    let vertex = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nVe 0.001 2 3 4 0 0 1001000 *\n+1 0 *";
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("saved.brp", vertex),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.semantic_annotations[0].value.is_none());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "drawing.dimension-geometry-not-transferred"));
}

#[test]
fn techdraw_arbitrary_tolerance_has_loss_and_position_has_one_owner() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects><ObjectData Count="1"><Object name="Dimension"><Properties Count="5"><Property name="ArbitraryTolerances" type="App::PropertyBool"><Bool value="true"/></Property><Property name="OverTolerance" type="App::PropertyQuantityConstraint"><Float value="0.2"/></Property><Property name="FormatSpecOverTolerance" type="App::PropertyString"><String value="LIMIT"/></Property><Property name="X" type="App::PropertyDistance"><Float value="7"/></Property><Property name="Y" type="App::PropertyDistance"><Float value="8"/></Property></Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(result.ir().model.drawings[0].position.is_none());
    let annotation = &result.ir().model.semantic_annotations[0];
    assert_eq!(annotation.position.unwrap().get(), [7.0, 8.0, 0.0]);
    assert!(annotation.upper_tolerance.is_none());
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "drawing.dimension-tolerance-not-transferred")
            .count(),
        2
    );
}

#[test]
fn techdraw_cosmetic_vertex_and_blank_title_field_have_losses() {
    let vertex = r##"<CosmeticVertex type="TechDraw::CosmeticVertex"><Point X="2" Y="3" Z="0"/><Extract value="0"/><HLRVisible value="1"/><Ref3D value="-1"/><IsCenter value="0"/><Cosmetic value="1"/><CosmeticLink value="-1"/><CosmeticTag value="12345678-1234-1234-1234-123456789abc"/><PermaPoint X="2" Y="3" Z="0"/><LinkGeom value="0"/><Color value="#000000"/><Size value="1"/><Style value="1"/><Visible value="1"/><Tag value="12345678-1234-1234-1234-123456789abc"/></CosmeticVertex>"##;
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="2"><Object type="TechDraw::DrawViewPart" name="View"/><Object type="TechDraw::DrawSVGTemplate" name="Template"/></Objects><ObjectData Count="2"><Object name="View"><Properties Count="1"><Property name="CosmeticVertexes" type="TechDraw::PropertyCosmeticVertexList"><CosmeticVertexList count="1">{vertex}</CosmeticVertexList></Property></Properties></Object><Object name="Template"><Properties Count="2"><Property name="PageResult" type="App::PropertyFileIncluded"><FileIncluded file="sheet.svg"/></Property><Property name="EditableTexts" type="App::PropertyMap"><Map count="1"><Item key=" " value="TEST"/></Map></Property></Properties></Object></ObjectData></Document>"#
    );
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("sheet.svg", br#"<svg width="20mm" height="30mm"/>"#),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    for code in [
        "drawing.cosmetic-vertex-not-transferred",
        "drawing.title-block-not-transferred",
    ] {
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == code));
    }
}
