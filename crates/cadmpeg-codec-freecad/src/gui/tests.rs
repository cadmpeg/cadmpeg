// SPDX-License-Identifier: Apache-2.0
//! GUI document transfer unit tests.

#![allow(clippy::doc_markdown)]

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
pub(crate) fn retains_ordered_document_level_gui_state() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1" active="UnrecognizedRootState">
 <ViewProviderData Count="0"/>
 <Camera settings="OrthographicCamera { position 1 2 3 orientation 0 0 1 0.25 }"/>
 <ClipPlane enabled="true" file="section.bin"/>
</Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("section.bin", b"section-state"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("GUI state");
    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let documents = namespace
        .arena_as::<crate::native::GuiDocumentRecord>("gui_documents")
        .expect("GUI documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].schema_version.as_deref(), Some("1"));
    assert_eq!(documents[0].attributes["active"], "UnrecognizedRootState");
    assert_eq!(
        documents[0]
            .states
            .iter()
            .map(|state| state.kind.as_str())
            .collect::<Vec<_>>(),
        ["Camera", "ClipPlane"]
    );
    assert_eq!(
        documents[0].states[0].attributes["settings"],
        "OrthographicCamera { position 1 2 3 orientation 0 0 1 0.25 }"
    );
    assert_eq!(documents[0].states[1].side_entries, ["section.bin"]);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let section = entries
        .iter()
        .find(|entry| entry.name == "section.bin")
        .expect("section asset");
    assert_eq!(section.referenced_by, [documents[0].states[1].id.clone()]);
    assert_eq!(result.ir().model.presentation_documents.len(), 1);
    let presentation = &result.ir().model.presentation_documents[0];
    assert_eq!(presentation.schema_version, Some(1));
    assert_eq!(presentation.active_view, None);
    let camera = presentation.camera.as_ref().expect("camera state");
    assert_eq!(camera.position, Some([1.0, 2.0, 3.0]));
    assert_eq!(camera.orientation, Some([0.0, 0.0, 1.0, 0.25]));
    assert_eq!(
        camera.properties["settings"],
        "OrthographicCamera { position 1 2 3 orientation 0 0 1 0.25 }"
    );
    assert_eq!(presentation.states[1].assets.len(), 1);
    assert!(presentation.states[1].assets[0].ends_with("section.bin"));
    assert!(result.ir().model.view_presentations.is_empty());
    assert!(crate::validate_native(result.ir()).is_empty());
    assert_valid_document(result.ir());

    let mut corrupted = result.ir().clone();
    corrupted.model.presentation_documents[0]
        .camera
        .as_mut()
        .expect("camera state")
        .orientation = Some([0.0; 4]);
    assert!(cadmpeg_ir::validate_neutral(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid document presentation state"));
}

#[test]
fn requires_one_camera_in_schema_one_gui_document() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    for camera_records in [
        "",
        r#"<Camera settings="first"/><Camera settings="second"/>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/>{camera_records}</Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document.as_bytes()),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("schema-one camera cardinality");

        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
                if message.contains("schema 1 requires one Camera record")
        ));
    }
}

#[test]
fn a_foreign_gui_schema_uses_the_schema_one_vocabulary() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui = br#"<Document SchemaVersion="2"><ViewProviderData Count="0"/><Camera settings="OrthographicCamera { position 1 2 3 orientation 0 0 1 0 }"/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("foreign GUI schema with schema-1 vocabulary");

    assert_eq!(result.ir().model.presentation_documents.len(), 1);
    assert_eq!(
        result.ir().model.presentation_documents[0].schema_version,
        None
    );
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "source.gui-schema-unverified")
        .expect("GUI schema warning");
    assert_eq!(loss.severity, cadmpeg_ir::Severity::Warning);
    assert!(loss.message.contains("declares schema 2"));
    assert!(loss.message.contains("schema-1 vocabulary"));
}

#[test]
fn a_noncanonical_gui_schema_one_declaration_is_unverified() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui = br#"<Document SchemaVersion="01"><ViewProviderData Count="0"/><Camera settings="OrthographicCamera { position 1 2 3 orientation 0 0 1 0 }"/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("noncanonical GUI schema with schema-1 vocabulary");

    let namespace = result.ir().native.namespace("fcstd").expect("native");
    let documents = namespace
        .arena_as::<crate::native::GuiDocumentRecord>("gui_documents")
        .expect("GUI documents");
    assert_eq!(documents[0].schema_version.as_deref(), Some("01"));
    assert_eq!(
        result.ir().model.presentation_documents[0].schema_version,
        None
    );
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "source.gui-schema-unverified")
        .expect("GUI schema warning");
    assert!(loss.message.contains("declares schema 01"));
}

#[test]
fn a_broken_foreign_gui_schema_degrades_to_the_default_graph() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui = br#"<Document SchemaVersion="2"><ViewProviderData Count="0"/><Camera settings="first"/><Camera settings="second"/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("broken foreign GUI schema degrades");

    assert!(result.ir().model.presentation_documents.is_empty());
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "source.gui-schema-unverified")
        .expect("GUI schema warning");
    assert_eq!(loss.severity, cadmpeg_ir::Severity::Warning);
    assert!(loss
        .message
        .contains("declared schema 2 is the probable cause"));
}

#[test]
fn a_failed_foreign_gui_parse_does_not_apply_staged_appearances() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="2"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="2">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3424269311"/></Property>
<Property name="LineWidth" type="App::PropertyFloatConstraint"><Float value="-1"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("broken foreign GUI schema degrades");

    assert!(result.ir().model.appearances.is_empty());
    assert!(result.ir().model.appearance_bindings.is_empty());
    assert!(result.ir().model.presentation_documents.is_empty());
    assert!(result.ir().model.view_presentations.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "source.gui-schema-unverified"));
}

#[test]
fn rejects_invalid_schema_one_camera_values() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui_documents = [
        r#"<Document schemaVersion="1"><ViewProviderData Count="0"/></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera/></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera settings="OrthographicCamera { position NaN 1 2 orientation 0 0 1 0 }"/></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera settings="OrthographicCamera { position 1 2 3 orientation NaN 0 0 1 }"/></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera settings="OrthographicCamera { position 1 2 3 orientation 0 0 0 0 }"/></Document>"#,
        r#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><Camera settings="not a camera"/></Document>"#,
    ];
    for gui in gui_documents {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid schema-one camera value");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn ignores_non_authoritative_camera_descendant_values() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="0"><!-- no providers --></ViewProviderData>
<Camera settings="" orientation="9 9 9 9"><Position x="NaN" y="1" z="2"/><Position x="4" y="5" z="6"/></Camera></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("non-authoritative camera descendants");
    let camera = result.ir().model.presentation_documents[0]
        .camera
        .as_ref()
        .expect("camera state");
    assert_eq!(camera.position, None);
    assert_eq!(camera.orientation, None);
    assert_eq!(camera.properties["orientation"], "9 9 9 9");
}

#[test]
fn rejects_duplicate_camera_settings_fields() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="0"/><ObjectData Count="0"/></Document>"#;
    for settings in [
        "OrthographicCamera { position 1 2 3 position 4 5 6 orientation 0 0 1 0 }",
        "PerspectiveCamera { position 1 2 3 orientation 0 0 1 0 orientation 0 1 0 0.5 }",
    ] {
        let gui = format!(
            "<Document SchemaVersion=\"1\"><ViewProviderData Count=\"0\"/><Camera settings=\"{settings}\"/></Document>"
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("duplicate camera settings field");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(message))
                if message.contains("multiple")
        ));
    }
}

#[test]
fn keeps_registered_non_presentation_properties_native() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="20">
<Property name="ShowInTree" type="App::PropertyBool"><Bool value="false"/></Property>
<Property name="OnTopWhenSelected" type="App::PropertyEnumeration"><Integer value="2"/></Property>
<Property name="BoundingBox" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="Selectable" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="DrawStyle" type="App::PropertyEnumeration"><Integer value="3"/></Property>
<Property name="LineMaterial" type="App::PropertyMaterial"><PropertyMaterial ambientColor="1" diffuseColor="2" specularColor="3" emissiveColor="4" shininess="0.5" transparency="0.25"/></Property>
<Property name="PointMaterial" type="App::PropertyMaterial"><PropertyMaterial ambientColor="5" diffuseColor="6" specularColor="7" emissiveColor="8" shininess="0.75" transparency="0.125"/></Property>
<Property name="ShowPlacement" type="App::PropertyBool"><Bool value="true"/></Property>
<Property name="TransformOrigin" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="AngularDeflection" type="App::PropertyAngle"><Float value="28.65"/></Property>
<Property name="Deviation" type="App::PropertyFloatConstraint"><Float value="0.2"/></Property>
<Property name="Visibility" type="App::PropertyBool"><Bool value="false"/></Property>
<Property name="DisplayMode" type="App::PropertyEnumeration"><Integer value="3"/></Property>
<Property name="SelectionStyle" type="App::PropertyEnumeration"><Integer value="1"/></Property>
<Property name="LineWidth" type="App::PropertyFloatConstraint"><Float value="3.5"/></Property>
<Property name="PointSize" type="App::PropertyFloatConstraint"><Float value="4.5"/></Property>
<Property name="Transparency" type="App::PropertyPercent"><Integer value="25"/></Property>
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3424269311"/></Property>
<Property name="LineColor" type="App::PropertyColor"><PropertyColor value="447958527"/></Property>
<Property name="PointColor" type="App::PropertyColor"><PropertyColor value="1144201983"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("registered GUI properties");
    let view = &result.ir().model.view_presentations[0];
    assert_eq!(view.visible, Some(false));
    assert_eq!(view.display_mode.as_deref(), Some("3"));
    assert_eq!(view.selection_style.as_deref(), Some("1"));
    assert_eq!(view.line_width, Some(3.5));
    assert_eq!(view.point_size, Some(4.5));
    for name in [
        "ShowInTree",
        "OnTopWhenSelected",
        "BoundingBox",
        "Selectable",
        "DrawStyle",
        "LineMaterial",
        "PointMaterial",
        "ShowPlacement",
        "TransformOrigin",
        "AngularDeflection",
        "Deviation",
        "Transparency",
        "ShapeColor",
        "LineColor",
        "PointColor",
    ] {
        assert!(
            view.properties.contains_key(name),
            "missing native property {name}"
        );
    }
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(properties.len(), 20);
    assert!(properties.iter().all(|property| {
        crate::gui::has_registered_property_grammar(&property.name, &property.type_name)
    }));
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn retains_topology_color_count_mismatches_and_reports_losses() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1">
<Property name="Shape" type="Part::PropertyPartShape">
<Part ElementMap="1.0" file="Shape.brp"/>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="4">
1 PostfixCount 0 MapCount 1
ElementMap 1 1 3
Face ChildCount 0 NameCount 2
0
;FaceStable.0.a 0
Edge ChildCount 0 NameCount 3
0
;EdgeStable1.0.a 0
;EdgeStable2.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 1 0
1 1 0 -1 0
Curves 2
1 0 0 0 1 0 0
1 1 0 0 -1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let color_list = |colors: &[u32]| {
        let mut bytes = Vec::with_capacity(4 + colors.len() * 4);
        bytes.extend_from_slice(&(colors.len() as u32).to_le_bytes());
        for color in colors {
            bytes.extend_from_slice(&color.to_le_bytes());
        }
        bytes
    };

    for (mismatched_property, mismatched_count, kind, expected_valid_bindings) in [
        ("DiffuseColor", 2_usize, "Face", 4_usize),
        ("LineColorArray", 3_usize, "Edge", 3_usize),
        ("PointColorArray", 3_usize, "Vertex", 3_usize),
        ("ShapeAppearance", 2_usize, "Face", 5_usize),
        ("ShapeAppearance", 0_usize, "Face", 5_usize),
    ] {
        let shape_appearance_property = if mismatched_property == "ShapeAppearance" {
            r#"<Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="ShapeAppearance" version="2"/></Property>"#
        } else {
            ""
        };
        let property_count = if mismatched_property == "ShapeAppearance" {
            5
        } else {
            4
        };
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Shape"><Properties Count="{property_count}">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3435973632"/></Property>
<Property name="DiffuseColor" type="App::PropertyColorList"><ColorList file="DiffuseColor"/></Property>
<Property name="LineColorArray" type="App::PropertyColorList"><ColorList file="LineColorArray"/></Property>
<Property name="PointColorArray" type="App::PropertyColorList"><ColorList file="PointColorArray"/></Property>
{shape_appearance_property}
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let face_colors = color_list(if mismatched_property == "DiffuseColor" {
            &[0xff00_00ff, 0x00ff_00ff][..mismatched_count]
        } else {
            &[0xff00_00ff][..]
        });
        let edge_colors = color_list(if mismatched_property == "LineColorArray" {
            &[0xff00_00ff, 0x00ff_00ff, 0x0000_ffff][..mismatched_count]
        } else {
            &[0xff00_00ff][..]
        });
        let point_colors = color_list(if mismatched_property == "PointColorArray" {
            &[0xff00_00ff, 0x00ff_00ff, 0x0000_ffff][..mismatched_count]
        } else {
            &[0xff00_00ff][..]
        });
        let shape_material_count = if mismatched_property == "ShapeAppearance" {
            mismatched_count
        } else {
            0
        };
        let mut shape_materials = (shape_material_count as u32).to_le_bytes().to_vec();
        for diffuse in [0xff00_00ff, 0x00ff_00ff]
            .into_iter()
            .take(shape_material_count)
        {
            for packed in [0_u32, diffuse, 0_u32, 0_u32] {
                shape_materials.extend_from_slice(&packed.to_le_bytes());
            }
            shape_materials.extend_from_slice(&0.0_f32.to_le_bytes());
            shape_materials.extend_from_slice(&0.0_f32.to_le_bytes());
        }
        let mut entries = vec![
            ("Document.xml", document.as_bytes()),
            ("GuiDocument.xml", gui.as_bytes()),
            ("DiffuseColor", face_colors.as_slice()),
            ("LineColorArray", edge_colors.as_slice()),
            ("PointColorArray", point_colors.as_slice()),
            ("Shape.brp", brep),
        ];
        if mismatched_property == "ShapeAppearance" {
            entries.push(("ShapeAppearance", shape_materials.as_slice()));
        }
        let result = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&entries)),
                &DecodeOptions::default(),
            )
            .expect("producer-accepted topology color mismatch");
        assert_eq!(result.report().losses.len(), 1);
        let loss = &result.report().losses[0];
        assert_eq!(
            loss.code.local_code(),
            "appearance.topology-color-count-mismatch"
        );
        assert_eq!(loss.severity, cadmpeg_ir::Severity::Warning);
        assert!(loss.message.contains(kind));
        assert!(loss.provenance.as_ref().is_some_and(|source| {
            source.stream() == Some("GuiDocument.xml") && source.offset > 0
        }));
        assert_eq!(
            result
                .ir()
                .model
                .appearance_bindings
                .iter()
                .filter(|binding| binding.channels.contains_key("precedence"))
                .count(),
            expected_valid_bindings
        );
        let properties = result
            .ir()
            .native
            .namespace("fcstd")
            .expect("namespace")
            .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
            .expect("GUI properties");
        assert!(properties.iter().any(|property| {
            property.name == mismatched_property && property.side_entries == [mismatched_property]
        }));
        assert!(crate::validate_native(result.ir()).is_empty());
    }
}

#[test]
fn rejects_ambiguous_gui_containers_and_names() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    for gui in [
        br#"<Document SchemaVersion="1"><ViewProviderData Count="0"/><ViewProviderData Count="0"/><Camera settings=""/></Document>"#.as_slice(),
        br#"<Document SchemaVersion="1"><ViewProviderData Count="2"><ViewProvider name="Model"><Properties Count="0"/></ViewProvider><ViewProvider name="Model"><Properties Count="0"/></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="0"/><Properties Count="0"/></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="2"><Property name="State" type="Vendor::PropertyState"><Value/></Property><Property name="State" type="Vendor::PropertyState"><Value/></Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
    ] {
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("ambiguous GUI graph");
        assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))));
    }
}

#[test]
fn gui_property_counts_ignore_nested_extension_properties() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="0"/><Extension name="Nested"><Properties Count="1"><Property name="NestedValue" type="App::PropertyString"><String value="kept by extension"/></Property></Properties></Extension></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("nested extension properties do not alter the provider's direct count");
    let native = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("FCStd namespace");
    assert_eq!(
        native
            .arena_as::<crate::native::ObjectRecord>("objects")
            .expect("objects")
            .len(),
        1
    );
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn rejects_malformed_registered_gui_property_values() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="LineWidth" type="App::PropertyFloatConstraint"><Integer value="2"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("mismatched GUI value tag");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn validates_gui_link_value_grammars() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="8">
<Property name="Link" type="App::PropertyLink"><Link value=""/></Property>
<Property name="LinkList" type="App::PropertyLinkList"><LinkList count="2"><Link value=""/><Link value=""/></LinkList></Property>
<Property name="LinkSub" type="App::PropertyLinkSub"><LinkSub value="" count="1"><Sub value="Face1"/></LinkSub></Property>
<Property name="LinkSubList" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="" sub="Face1"/></LinkSubList></Property>
<Property name="XLink" type="App::PropertyXLink"><XLink name="" file=""/></Property>
<Property name="XLinkSub" type="App::PropertyXLinkSub"><XLink name="" file="" sub="Face1"/></Property>
<Property name="XLinkSubList" type="App::PropertyXLinkSubList"><XLinkSubList count="1"><XLink name="" file="" sub="Face1"/></XLinkSubList></Property>
<Property name="PlacementLink" type="App::PropertyPlacementLink"><Link value=""/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid GUI link grammars");

    for invalid in [
        r#"<Property name="Link" type="App::PropertyLink"><Link value=""><Nested/></Link></Property>"#,
        r#"<Property name="LinkList" type="App::PropertyLinkList"><LinkList count="1"><Link value=""/><Link value=""/></LinkList></Property>"#,
        r#"<Property name="LinkSub" type="App::PropertyLinkSub"><LinkSub value="" count="1"><Sub value="Face1"><Nested/></Sub></LinkSub></Property>"#,
        r#"<Property name="LinkSubList" type="App::PropertyLinkSubList"><LinkSubList count="1"><Link obj="" sub="Face1"><Nested/></Link></LinkSubList></Property>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid GUI link grammar");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn validates_gui_constraint_attribute_grammars() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="3">
<Property name="IntegerConstraint" type="App::PropertyIntegerConstraint"><Integer value="4" min="0" max="10" step="2"/></Property>
<Property name="FloatConstraint" type="App::PropertyFloatConstraint"><Float value="1.5" min="0" max="5" step="0.5"/></Property>
<Property name="Length" type="App::PropertyLength"><Float value="12" min="0" max="25" step="0.5"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid GUI constraint attributes");

    for invalid in [
        r#"<Property name="IntegerConstraint" type="App::PropertyIntegerConstraint"><Integer value="4" min="zero"/></Property>"#,
        r#"<Property name="FloatConstraint" type="App::PropertyFloatConstraint"><Float value="1.5" step="not-a-float"/></Property>"#,
        r#"<Property name="Length" type="App::PropertyLength"><Float value="12" max="NaN"/></Property>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid GUI constraint attribute");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn validates_gui_in_memory_list_grammars() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="5">
<Property name="Flags" type="App::PropertyBoolList"><BoolList value="101"/></Property>
<Property name="Names" type="App::PropertyStringList"><StringList count="2"><String value="alpha"/><String value="beta"/></StringList></Property>
<Property name="Values" type="App::PropertyIntegerList"><IntegerList count="2"><I v="2"/><I v="4"/></IntegerList></Property>
<Property name="UniqueValues" type="App::PropertyIntegerSet"><IntegerSet count="2"><I v="1"/><I v="4"/></IntegerSet></Property>
<Property name="Mapping" type="App::PropertyMap"><Map count="2"><Item key="alpha" value="one"/><Item key="beta" value="two"/></Map></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid GUI in-memory lists");

    for invalid in [
        r#"<Property name="Flags" type="App::PropertyBoolList"><BoolList value="101"><Nested/></BoolList></Property>"#,
        r#"<Property name="Names" type="App::PropertyStringList"><StringList count="1"><String value="alpha"><Nested/></String></StringList></Property>"#,
        r#"<Property name="Values" type="App::PropertyIntegerList"><IntegerList count="1"><I v="2"><Nested/></I></IntegerList></Property>"#,
        r#"<Property name="UniqueValues" type="App::PropertyIntegerSet"><IntegerSet count="1"><I v="1"><Nested/></I></IntegerSet></Property>"#,
        r#"<Property name="Mapping" type="App::PropertyMap"><Map count="1"><Item key="alpha" value="one"><Nested/></Item></Map></Property>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("nested GUI list value");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn accepts_and_validates_gui_custom_enumerations() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="2"><Property name="Pattern" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="None"/><Enum value="Cross"/></CustomEnumList></Property><Property name="ChildViewProvider" type="App::PropertyPersistentObject"><String value=""/><PersistentObject><State value="retained"/></PersistentObject></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("custom GUI enumeration");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let enumeration = properties
        .iter()
        .find(|property| property.name == "Pattern")
        .expect("enumeration");
    let persistent = properties
        .iter()
        .find(|property| property.name == "ChildViewProvider")
        .expect("persistent object");
    assert_eq!(enumeration.values.len(), 4);
    assert_eq!(persistent.values[0].tag, "String");
    assert_eq!(persistent.values[1].tag, "PersistentObject");
    assert_eq!(persistent.values[2].tag, "State");

    for invalid in [
        br#"<Integer value="1"/><CustomEnumList count="0"/>"#.as_slice(),
        br#"<Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="None"/></CustomEnumList>"#.as_slice(),
    ] {
        let gui = [
            br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1"><Property name="Pattern" type="App::PropertyEnumeration">"#.as_slice(),
            invalid,
            br#"</Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        ]
        .concat();
        assert!(FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", &gui),
                ])),
                &DecodeOptions::default(),
            )
            .is_err());
    }
}

#[test]
fn validates_sketcher_visual_layer_list_with_the_producer_type_token() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Sketch"><Properties Count="1"><Property name="VisualLayerList" type="BadType">
<VisualLayerList count="2"><VisualLayer visible="true" linePattern="65535" lineWidth="3.0"/><VisualLayer visible="false" linePattern="32382" lineWidth="1.5"/></VisualLayerList>
</Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("Sketcher visual layers");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let property = properties
        .iter()
        .find(|property| property.name == "VisualLayerList")
        .expect("visual layers");
    assert_eq!(property.type_name, "BadType");
    assert_eq!(
        property
            .values
            .iter()
            .map(|value| value.tag.as_str())
            .collect::<Vec<_>>(),
        ["VisualLayerList", "VisualLayer", "VisualLayer"]
    );
    let logical = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    let span = logical
        .iter()
        .find(|span| span.owner.as_deref() == Some(property.id.as_str()))
        .expect("visual layer span");
    assert_eq!(span.classification, "typed");
    assert!(crate::validate_native(result.ir()).is_empty());

    for value in [
        br#"<VisualLayerList count="1"><VisualLayer visible="maybe" linePattern="1" lineWidth="1"/></VisualLayerList>"#.as_slice(),
        br#"<VisualLayerList count="2"><VisualLayer visible="true" linePattern="1" lineWidth="1"/></VisualLayerList>"#.as_slice(),
        br#"<VisualLayerList count="1"><VisualLayer visible="true" linePattern="1" lineWidth="NaN"/></VisualLayerList>"#.as_slice(),
    ] {
        let gui = [
            br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Sketch"><Properties Count="1"><Property name="VisualLayerList" type="BadType">"#.as_slice(),
            value,
            br#"</Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#.as_slice(),
        ]
        .concat();
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", &gui),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid visual layer list");
        assert!(matches!(error, cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))));
    }
}

#[test]
fn validates_dynamic_gui_property_registry_and_side_lists() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="12">
<Property name="FloatList" type="App::PropertyFloatList"><FloatList file="FloatList"/></Property>
<Property name="VectorList" type="App::PropertyVectorList"><VectorList file="VectorList"/></Property>
<Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="PlacementList"/></Property>
<Property name="BoolList" type="App::PropertyBoolList"><BoolList value="101"/></Property>
<Property name="IntegerSet" type="App::PropertyIntegerSet"><IntegerSet count="3"><I v="1"/><I v="4"/><I v="9"/></IntegerSet></Property>
<Property name="Strings" type="App::PropertyStringList"><StringList count="2"><String value="alpha"/><String value="beta"/></StringList></Property>
<Property name="Map" type="App::PropertyMap"><Map count="2"><Item key="alpha" value="one"/><Item key="beta" value="two"/></Map></Property>
<Property name="Matrix" type="App::PropertyMatrix"><PropertyMatrix a11="1" a12="0" a13="0" a14="0" a21="0" a22="1" a23="0" a24="0" a31="0" a32="0" a33="1" a34="0" a41="0" a42="0" a43="0" a44="1"/></Property>
<Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
<Property name="Rotation" type="App::PropertyRotation"><PropertyRotation A="0" Ox="0" Oy="0" Oz="1"/></Property>
<Property name="Uuid" type="App::PropertyUUID"><Uuid value="01234567-89ab-cdef-0123-456789abcdef"/></Property>
<Property name="Path" type="App::PropertyPath"><Path value="/var/tmp/path"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let mut float_list = Vec::new();
    float_list.extend_from_slice(&2_u32.to_le_bytes());
    float_list.extend_from_slice(&1.25_f64.to_le_bytes());
    float_list.extend_from_slice(&2.5_f64.to_le_bytes());
    let mut vector_list = Vec::new();
    vector_list.extend_from_slice(&1_u32.to_le_bytes());
    for value in [1.0_f64, 2.0, 3.0] {
        vector_list.extend_from_slice(&value.to_le_bytes());
    }
    let mut placement_list = Vec::new();
    placement_list.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
        placement_list.extend_from_slice(&value.to_le_bytes());
    }
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("FloatList", &float_list),
                ("VectorList", &vector_list),
                ("PlacementList", &placement_list),
            ])),
            &DecodeOptions::default(),
        )
        .expect("dynamic GUI registry");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(properties.len(), 12);
    assert!(properties.iter().all(|property| {
        crate::gui::has_registered_property_grammar(&property.name, &property.type_name)
    }));
    assert!(crate::validate_native(result.ir()).is_empty());

    let bad_float_list = [0_u32.to_le_bytes().as_slice(), &[0xff]].concat();
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("FloatList", &bad_float_list),
                ("VectorList", &vector_list),
                ("PlacementList", &placement_list),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("trailing dynamic float-list bytes");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn rejects_gui_side_entries_owned_by_nested_values() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let empty_count = 0_u32.to_le_bytes();
    let valid_gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="7">
<Property name="Floats" type="App::PropertyFloatList"><FloatList file="Floats"/></Property>
<Property name="Vectors" type="App::PropertyVectorList"><VectorList file="Vectors"/></Property>
<Property name="Placements" type="App::PropertyPlacementList"><PlacementList file="Placements"/></Property>
<Property name="Colors" type="App::PropertyColorList"><ColorList file="Colors"/></Property>
<Property name="Materials" type="App::PropertyMaterialList"><MaterialList file="Materials" version="3"/></Property>
<Property name="Included" type="App::PropertyFileIncluded"><FileIncluded file="Included"/></Property>
<Property name="Inline" type="App::PropertyFileIncluded"><FileIncluded data=""/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", valid_gui),
                ("Floats", &empty_count),
                ("Vectors", &empty_count),
                ("Placements", &empty_count),
                ("Colors", &empty_count),
                ("Materials", &empty_count),
                ("Included", b"direct-side-entry"),
            ])),
            &DecodeOptions::default(),
        )
        .expect("direct GUI side-entry references");

    for invalid in [
        r#"<Property name="Floats" type="App::PropertyFloatList"><FloatList file=""><Nested file="Floats"/></FloatList></Property>"#,
        r#"<Property name="Vectors" type="App::PropertyVectorList"><VectorList file=""><Nested file="Vectors"/></VectorList></Property>"#,
        r#"<Property name="Placements" type="App::PropertyPlacementList"><PlacementList file=""><Nested file="Placements"/></PlacementList></Property>"#,
        r#"<Property name="Colors" type="App::PropertyColorList"><ColorList file=""><Nested file="Colors"/></ColorList></Property>"#,
        r#"<Property name="Materials" type="App::PropertyMaterialList"><MaterialList file="" version="3"><Nested file="Materials"/></MaterialList></Property>"#,
        r#"<Property name="Included" type="App::PropertyFileIncluded"><FileIncluded file=""><Nested file="Included"/></FileIncluded></Property>"#,
        r#"<Property name="Included" type="App::PropertyFileIncluded"><FileIncluded file="Included" data="inline"/></Property>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                    ("Floats", &empty_count),
                    ("Vectors", &empty_count),
                    ("Placements", &empty_count),
                    ("Colors", &empty_count),
                    ("Materials", &empty_count),
                    ("Included", b"nested-side-entry"),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("nested GUI side-entry reference");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn validates_gui_mesh_and_points_value_grammars() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="2">
<Property name="Mesh" type="Mesh::PropertyMeshKernel"><Mesh file="MeshPayload"/></Property>
<Property name="Points" type="Points::PropertyPointKernel"><Points file="PointsPayload" mtrx="1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("MeshPayload", b"mesh"),
                ("PointsPayload", b"points"),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid GUI mesh and points roots");

    for invalid in [
        r#"<Property name="Mesh" type="Mesh::PropertyMeshKernel"><Mesh file=""><Nested file="MeshPayload"/></Mesh></Property>"#,
        r#"<Property name="Points" type="Points::PropertyPointKernel"><Points file="PointsPayload"><Nested file="Other"/></Points></Property>"#,
        r#"<Property name="Mesh" type="Mesh::PropertyMeshKernel"><Mesh file="MeshPayload"/><Mesh file="Other"/></Property>"#,
        r#"<Property name="Points" type="Points::PropertyPointKernel"><Mesh file="PointsPayload"/></Property>"#,
        r#"<Property name="Points" type="Points::PropertyPointKernel"><Points file="PointsPayload" mtrx="1 0 0"/></Property>"#,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                    ("MeshPayload", b"mesh"),
                    ("PointsPayload", b"points"),
                    ("Other", b"other"),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid GUI mesh or points root");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn validates_gui_techdraw_geom_format_list_grammar() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br##"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1"><Property name="Formats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="3"><GeomFormat type="TechDraw::GeomFormat"><GeomIndex value="0"/><Style value="2"/><Weight value="0.7"/><Color value="#FF0000"/><Visible value="1"/><LineNumber value="2"/></GeomFormat><GeomFormat type="TechDraw::GeomFormat"><GeomIndex value="1"/><Style value="1"/><Weight value="0.5"/><Color value="#000000"/><Visible value="0"/></GeomFormat><GeomFormat type="TechDraw::GeomFormat"><GeomIndex value="2"/><Style value="1"/><Weight value="0.5"/><Color value="#000000"/><Visible value="1"/><ISOLineNumber value="3"/></GeomFormat></GeomFormatList></Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"##;
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid TechDraw GeomFormatList");

    for invalid in [
        r#"<Property name="Formats" type="TechDraw::PropertyGeomFormatList"><Wrong count="0"/></Property>"#,
        r#"<Property name="Formats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="1"/></Property>"#,
        r#"<Property name="Formats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="1"><Other type="TechDraw::GeomFormat"/></GeomFormatList></Property>"#,
        r##"<Property name="Formats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="1"><GeomFormat type="App::PropertyString"><GeomIndex value="0"/><Style value="2"/><Weight value="0.7"/><Color value="#FF0000"/><Visible value="1"/></GeomFormat></GeomFormatList></Property>"##,
        r##"<Property name="Formats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="1"><GeomFormat type="TechDraw::GeomFormat"><GeomIndex value="0"><Nested/></GeomIndex><Style value="2"/><Weight value="0.7"/><Color value="#FF0000"/><Visible value="1"/></GeomFormat></GeomFormatList></Property>"##,
        r##"<Property name="Formats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="1"><GeomFormat type="TechDraw::GeomFormat"><GeomIndex value="0"/><Style value="2"/><Weight value="nan"/><Color value="#FF0000"/><Visible value="1"/></GeomFormat></GeomFormatList></Property>"##,
        r#"<Property name="Formats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="1"><GeomFormat type="TechDraw::GeomFormat"><GeomIndex value="0"/><Style value="2"/><Weight value="0.7"/><Color value="red"/><Visible value="1"/></GeomFormat></GeomFormatList></Property>"#,
        r##"<Property name="Formats" type="TechDraw::PropertyGeomFormatList"><GeomFormatList count="1"><GeomFormat type="TechDraw::GeomFormat"><GeomIndex value="0"/><Style value="2"/><Weight value="0.7"/><Color value="#FF0000"/><Visible value="1"/><Unexpected value="2"/></GeomFormat></GeomFormatList></Property>"##,
    ] {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid TechDraw GeomFormatList");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn validates_gui_techdraw_cosmetic_vertex_list_grammar() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let record = r##"<CosmeticVertex type="TechDraw::CosmeticVertex"><Point X="1" Y="-2" Z="0"/><Extract value="0"/><HLRVisible value="1"/><Ref3D value="-1"/><IsCenter value="0"/><Cosmetic value="1"/><CosmeticLink value="-1"/><CosmeticTag value="58140d97-21b3-402f-9449-9ab33eaf2ac7"/><PermaPoint X="1" Y="-2" Z="0"/><LinkGeom value="4"/><Color value="#000000"/><Size value="2.1"/><Style value="1"/><Visible value="1"/><Tag value="58140d97-21b3-402f-9449-9ab33eaf2ac7"/></CosmeticVertex>"##;
    let legacy_record = r##"<CosmeticVertex type="TechDraw::CosmeticVertex"><Point X="2" Y="3" Z="4"/><Extract value="2"/><HLRVisible value="0"/><Ref3D value="7"/><IsCenter value="1"/><Cosmetic value="1"/><CosmeticLink value="3"/><CosmeticTag value="01234567-89ab-cdef-0123-456789abcdef"/><VertexTag value="01234567-89ab-cdef-0123-456789abcdef"/><PermaPoint X="2" Y="3" Z="4"/><LinkGeom value="8"/><Color value="#11223344"/><Size value="0.5"/><Style value="2"/><Visible value="false"/><Tag value="01234567-89ab-cdef-0123-456789abcdef"/></CosmeticVertex>"##;
    let gui = format!(
        r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1"><Property name="CosmeticVertexes" type="TechDraw::PropertyCosmeticVertexList"><CosmeticVertexList count="2">{record}{legacy_record}</CosmeticVertexList></Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
    );
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui.as_bytes()),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid TechDraw CosmeticVertexList");

    let property = |value: &str| {
        format!(
            r#"<Property name="CosmeticVertexes" type="TechDraw::PropertyCosmeticVertexList">{value}</Property>"#
        )
    };
    let invalid = [
        property(r#"<Wrong count="0"/>"#),
        property(r#"<CosmeticVertexList count="1"/>"#),
        property(
            r#"<CosmeticVertexList count="1"><Other type="TechDraw::CosmeticVertex"/></CosmeticVertexList>"#,
        ),
        property(
            r#"<CosmeticVertexList count="1"><CosmeticVertex type="App::PropertyString"/></CosmeticVertexList>"#,
        ),
        property(
            r#"<CosmeticVertexList count="1"><CosmeticVertex type="TechDraw::CosmeticVertex"><Point X="1" Y="-2" Z="0"><Nested/></Point></CosmeticVertex></CosmeticVertexList>"#,
        ),
        property(&record.replace("<Extract", "<Wrong/><Extract")),
        property(&record.replace("X=\"1\" Y=\"-2\"", "X=\"nan\" Y=\"-2\"")),
        property(&record.replace("<Extract value=\"0\"", "<Extract value=\"bad\"")),
        property(&record.replace("<Visible value=\"1\"", "<Visible value=\"maybe\"")),
        property(&record.replace("<Color value=\"#000000\"", "<Color value=\"red\"")),
        property(&record.replace(
            "<Tag value=\"58140d97-21b3-402f-9449-9ab33eaf2ac7\"",
            "<Tag value=\"not-a-uuid\"",
        )),
        property(&record.replace("</CosmeticVertex>", "<Extra/></CosmeticVertex>")),
    ];
    for invalid in invalid {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid TechDraw CosmeticVertexList");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn validates_gui_techdraw_cosmetic_edge_list_grammar() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let generic = r##"<CosmeticEdge type="TechDraw::CosmeticEdge"><Style value="2"/><Weight value="0.5"/><Color value="#000000"/><Visible value="1"/><GeometryType value="7"/><GeomType value="7"/><ExtractType value="0"/><EdgeClass value="5"/><HLRVisible value="1"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="1"/><Source value="1"/><SourceIndex value="-1"/><CosmeticTag value="0fbf303f-73ae-4c0f-875c-8298b16ec5b0"/><Points PointsCount="2"><Point X="0" Y="0" Z="0"/><Point X="5" Y="-3" Z="0"/></Points><LineNumber value="2"/></CosmeticEdge>"##;
    let circle = r##"<CosmeticEdge type="TechDraw::CosmeticEdge"><Style value="2"/><Weight value="0.5"/><Color value="#11223344"/><Visible value="false"/><GeometryType value="1"/><GeomType value="1"/><ExtractType value="0"/><EdgeClass value="5"/><HLRVisible value="1"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="1"/><Source value="1"/><SourceIndex value="-1"/><CosmeticTag value="d5801935-889a-4e16-87a7-f8ad58dc7de3"/><Center X="2" Y="-2" Z="0"/><Radius value="1.5"/></CosmeticEdge>"##;
    let arc = r##"<CosmeticEdge type="TechDraw::CosmeticEdge"><Style value="2"/><Weight value="0.5"/><Color value="#AABBCC"/><Visible value="1"/><GeometryType value="2"/><GeomType value="2"/><ExtractType value="0"/><EdgeClass value="0"/><HLRVisible value="1"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="0"/><Source value="0"/><SourceIndex value="-1"/><CosmeticTag value=""/><Center X="6" Y="2" Z="0"/><Radius value="1.25"/><Start X="7.1746" Y="2.4275" Z="0"/><End X="4.8254" Y="2.4275" Z="0"/><Middle X="6" Y="3.25" Z="0"/><StartAngle value="0.3491"/><EndAngle value="2.7925"/><Clockwise value="1"/><Large value="0"/><LineNumber value="2"/></CosmeticEdge>"##;
    let gui = format!(
        r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1"><Property name="CosmeticEdges" type="TechDraw::PropertyCosmeticEdgeList"><CosmeticEdgeList count="3">{generic}{circle}{arc}</CosmeticEdgeList></Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
    );
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui.as_bytes()),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid TechDraw CosmeticEdgeList");

    let property = |value: &str| {
        format!(
            r#"<Property name="CosmeticEdges" type="TechDraw::PropertyCosmeticEdgeList">{value}</Property>"#
        )
    };
    let invalid = vec![
        property(r#"<Wrong count="0"/>"#),
        property(&format!(
            r#"<CosmeticEdgeList count="2">{generic}</CosmeticEdgeList>"#
        )),
        property(&format!(
            r#"<CosmeticEdgeList count="1"><Other type="TechDraw::CosmeticEdge">{generic}</Other></CosmeticEdgeList>"#
        )),
        property(&generic.replace(
            "type=\"TechDraw::CosmeticEdge\"",
            "type=\"App::PropertyString\"",
        )),
        property(&generic.replace("<Style value=\"2\"/><Weight", "<Wrong value=\"2\"/><Weight")),
        property(&generic.replacen(
            "<GeometryType value=\"7\"/>",
            "<GeometryType value=\"3\"/>",
            1,
        )),
        property(&generic.replace("<GeomType value=\"7\"/>", "<GeomType value=\"1\"/>")),
        property(&generic.replace("PointsCount=\"2\"", "PointsCount=\"3\"")),
        property(&generic.replace("X=\"0\" Y=\"0\"", "X=\"nan\" Y=\"0\"")),
        property(&generic.replace("<Weight value=\"0.5\"", "<Weight value=\"nan\"")),
        property(&generic.replace("<Color value=\"#000000\"", "<Color value=\"red\"")),
        property(&generic.replace("<Visible value=\"1\"", "<Visible value=\"maybe\"")),
        property(&generic.replace("<LineNumber value=\"2\"/>", "<ISOLineNumber value=\"2\"/>")),
        property(&generic.replace(
            "<Point X=\"0\" Y=\"0\" Z=\"0\"/>",
            "<Point X=\"0\" Y=\"0\" Z=\"0\"><Nested/></Point>",
        )),
        property(&generic.replace(
            "<LineNumber value=\"2\"/>",
            "<LineNumber value=\"2\"/><Extra/>",
        )),
    ];
    for invalid in invalid {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid TechDraw CosmeticEdgeList");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn validates_gui_techdraw_center_line_list_grammar() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let generic = r##"<CenterLine type="TechDraw::CenterLine"><Start X="0" Y="0" Z="0"/><End X="0" Y="5" Z="0"/><Mode value="0"/><HShift value="0"/><VShift value="0"/><Rotate value="0"/><Extend value="0"/><Type value="1"/><Flip value="0"/><Faces FaceCount="0"></Faces><Edges EdgeCount="2"><Edge value="Edge1"/><Edge value="Edge2"/></Edges><CLPoints CLPointCount="0"></CLPoints><Style value="2"/><Weight value="0.5"/><Color value="#000000"/><Visible value="1"/><GeometryType value="7"/><GeomType value="7"/><ExtractType value="0"/><EdgeClass value="5"/><HLRVisible value="1"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="1"/><Source value="2"/><SourceIndex value="-1"/><CosmeticTag value="e8cc01e4-3dcb-4a63-a89c-94b92813253d"/><Points PointsCount="2"><Point X="0" Y="0" Z="0"/><Point X="0" Y="5" Z="0"/></Points><LineNumber value="2"/></CenterLine>"##;
    let circle = r##"<CenterLine type="TechDraw::CenterLine"><Start X="1" Y="0" Z="0"/><End X="1" Y="4" Z="0"/><Mode value="1"/><HShift value="0.25"/><VShift value="-0.5"/><Rotate value="0.1"/><Extend value="0.75"/><Type value="0"/><Flip value="1"/><Faces FaceCount="1"><Face value="Face1"/></Faces><Edges EdgeCount="0"></Edges><CLPoints CLPointCount="1"><CLPoint value="Vertex1"/></CLPoints><Style value="2"/><Weight value="0.5"/><Color value="#11223344"/><Visible value="false"/><GeometryType value="1"/><GeomType value="1"/><ExtractType value="0"/><EdgeClass value="5"/><HLRVisible value="1"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="1"/><Source value="2"/><SourceIndex value="-1"/><CosmeticTag value="circle-tag"/><Center X="2" Y="-2" Z="0"/><Radius value="1.5"/></CenterLine>"##;
    let arc = r##"<CenterLine type="TechDraw::CenterLine"><Start X="2" Y="0" Z="0"/><End X="2" Y="4" Z="0"/><Mode value="2"/><HShift value="0"/><VShift value="0"/><Rotate value="0"/><Extend value="0"/><Type value="2"/><Flip value="0"/><Faces FaceCount="0"></Faces><Edges EdgeCount="0"></Edges><CLPoints CLPointCount="0"></CLPoints><Style value="2"/><Weight value="0.5"/><Color value="#AABBCC"/><Visible value="1"/><GeometryType value="2"/><GeomType value="2"/><ExtractType value="0"/><EdgeClass value="0"/><HLRVisible value="1"/><Reversed value="0"/><Ref3D value="-1"/><Cosmetic value="0"/><Source value="2"/><SourceIndex value="-1"/><CosmeticTag value="arc-tag"/><Center X="6" Y="2" Z="0"/><Radius value="1.25"/><Start X="7.1746" Y="2.4275" Z="0"/><End X="4.8254" Y="2.4275" Z="0"/><Middle X="6" Y="3.25" Z="0"/><StartAngle value="0.3491"/><EndAngle value="2.7925"/><Clockwise value="1"/><Large value="0"/><ISOLineNumber value="2"/></CenterLine>"##;
    let gui = format!(
        r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1"><Property name="CenterLines" type="TechDraw::PropertyCenterLineList"><CenterLineList count="3">{generic}{circle}{arc}</CenterLineList></Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
    );
    FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui.as_bytes()),
            ])),
            &DecodeOptions::default(),
        )
        .expect("valid TechDraw CenterLineList");

    let property = |value: &str| {
        format!(
            r#"<Property name="CenterLines" type="TechDraw::PropertyCenterLineList">{value}</Property>"#
        )
    };
    let invalid = vec![
        property(r#"<Wrong count="0"/>"#),
        property(&format!(
            r#"<CenterLineList count="2">{generic}</CenterLineList>"#
        )),
        property(&generic.replace(
            "type=\"TechDraw::CenterLine\"",
            "type=\"App::PropertyString\"",
        )),
        property(&generic.replace("<Start X=", "<Wrong X=")),
        property(&generic.replace("<Mode value=\"0\"", "<Mode value=\"3\"")),
        property(&generic.replace("FaceCount=\"0\"", "FaceCount=\"1\"")),
        property(&generic.replace(
            "<Edge value=\"Edge1\"/>",
            "<Edge value=\"Edge1\"><Nested/></Edge>",
        )),
        property(&generic.replace("<HShift value=\"0\"", "<HShift value=\"nan\"")),
        property(&generic.replace("<Color value=\"#000000\"", "<Color value=\"red\"")),
        property(&generic.replacen("<GeomType value=\"7\"/>", "<GeomType value=\"1\"/>", 1)),
        property(&generic.replace("PointsCount=\"2\"", "PointsCount=\"3\"")),
        property(&generic.replace("<LineNumber value=\"2\"/>", "<Unexpected value=\"2\"/>")),
    ];
    for invalid in invalid {
        let gui = format!(
            r#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Model"><Properties Count="1">{invalid}</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#
        );
        let error = FcstdCodec
            .decode(
                &mut Cursor::new(archive_entries(&[
                    ("Document.xml", document),
                    ("GuiDocument.xml", gui.as_bytes()),
                ])),
                &DecodeOptions::default(),
            )
            .expect_err("invalid TechDraw CenterLineList");
        assert!(matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}

#[test]
fn validates_the_complete_loaded_dynamic_gui_registry() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="14">
<Property name="Precision" type="App::PropertyPrecision"><Float value="0.001"/></Property>
<Property name="VectorDistance" type="App::PropertyVectorDistance"><PropertyVector valueX="1" valueY="2" valueZ="3"/></Property>
<Property name="Position" type="App::PropertyPosition"><PropertyVector valueX="4" valueY="5" valueZ="6"/></Property>
<Property name="Direction" type="App::PropertyDirection"><PropertyVector valueX="0" valueY="0" valueZ="1"/></Property>
<Property name="PlacementLink" type="App::PropertyPlacementLink"><Link value=""/></Property>
<Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Length" expression="2"/></ExpressionEngine></Property>
<Property name="MaterialReference" type="Materials::PropertyMaterial"><PropertyMaterial uuid="6b80c8f7-cf5f-4e7d-a6e3-88b3cd1db5a3"/></Property>
<Property name="PartShape" type="Part::PropertyPartShape"><Part file="ProbeShape"/><ElementMap/></Property>
<Property name="GeometryList" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
<Property name="ShapeHistory" type="Part::PropertyShapeHistory"/>
<Property name="FilletEdges" type="Part::PropertyFilletEdges"><FilletEdges file="ProbeFillet"/></Property>
<Property name="ShapeCache" type="Part::PropertyShapeCache"/>
<Property name="TopoShapeList" type="Part::PropertyTopoShapeList"><ShapeList count="2"><TopoShape file="ProbeShapeList.0.brp"/><TopoShape file="ProbeShapeList.1.brp"/></ShapeList></Property>
<Property name="ConstraintList" type="Sketcher::PropertyConstraintList"><ConstraintList count="0"/></Property>
</Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("ProbeShape", b"shape-side-entry"),
                ("ProbeFillet", &0_u32.to_le_bytes()),
                ("ProbeShapeList.0.brp", b"shape-list-0"),
                ("ProbeShapeList.1.brp", b"shape-list-1"),
            ])),
            &DecodeOptions::default(),
        )
        .expect("complete dynamic GUI registry");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(properties.len(), 14);
    assert!(properties.iter().all(|property| {
        crate::gui::has_registered_property_grammar(&property.name, &property.type_name)
    }));
    assert!(crate::validate_native(result.ir()).is_empty());

    let logical = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    assert!(properties.iter().all(|property| {
        logical.iter().any(|span| {
            span.owner.as_deref() == Some(property.id.as_str()) && span.classification == "typed"
        })
    }));
}

#[test]
fn rejects_truncated_gui_material_list_payload() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ShapeAppearance" type="App::PropertyMaterialList"><MaterialList file="ShapeAppearance" version="3"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("ShapeAppearance", &1_u32.to_le_bytes()),
            ])),
            &DecodeOptions::default(),
        )
        .expect_err("truncated GUI material list");
    assert!(matches!(
        error,
        cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn retains_unregistered_gui_property_values_without_semantic_dispatch() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects><ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ExtensionState" type="Vendor::PropertyState"><VendorState mode="custom"><Nested value="kept"/></VendorState></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("unregistered GUI property");
    let properties = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(properties[0].type_name, "Vendor::PropertyState");
    assert_eq!(properties[0].values[0].tag, "VendorState");
    assert_eq!(properties[0].values[1].tag, "Nested");
}

#[test]
fn retains_unregistered_gui_side_entries_as_opaque_archive_members() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="ExtensionState" type="Vendor::PropertyState"><VendorState file="state.bin" mode="custom"><Nested value="kept"/></VendorState></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let payload = [
        0xd0, 0xc0, 0xb0, 0xa0, 0x00, 0x00, 0x01, 0x00, 0x7f, 0x45, 0x4c, 0x46,
    ];
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
                ("state.bin", &payload),
            ])),
            &DecodeOptions::default(),
        )
        .expect("unregistered GUI side entry");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let property = properties
        .iter()
        .find(|property| property.name == "ExtensionState")
        .expect("extension property");
    assert_eq!(property.side_entries, ["state.bin"]);

    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let entry = entries
        .iter()
        .find(|entry| entry.name == "state.bin")
        .expect("state entry");
    assert_eq!(
        entry.referenced_by.as_slice(),
        std::slice::from_ref(&property.id)
    );
    assert_eq!(entry.data, payload);

    let logical = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    let span = logical
        .iter()
        .find(|span| span.entry == entry.name)
        .expect("state span");
    assert_eq!(span.classification, "named_opaque");
    assert_eq!(span.owner.as_deref(), Some(entry.id.as_str()));
    assert!(crate::validate_native(result.ir()).is_empty());
}

#[test]
fn does_not_treat_gui_external_links_as_archive_members() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Model"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="1"><Property name="Remote" type="App::PropertyXLink"><XLink file="External.FCStd" name="Body"/></Property></Properties></ViewProvider>
</ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .expect("external GUI link");
    let namespace = result.ir().native.namespace("fcstd").expect("namespace");
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    let property = properties
        .iter()
        .find(|property| property.name == "Remote")
        .expect("external-link property");
    assert!(property.side_entries.is_empty());
    assert!(namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries")
        .iter()
        .all(|entry| entry.name != "External.FCStd"));
    assert!(crate::validate_native(result.ir()).is_empty());
}
