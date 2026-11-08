// SPDX-License-Identifier: Apache-2.0
//! GUI metadata recovery keeps geometry and a native GUI document carrier.

use super::*;
use crate::native::GuiDocumentRecord;
use crate::test_support::test_archive::{assert_valid_document, rewrite_entry, GEOMETRY};

#[test]
fn malformed_gui_metadata_keeps_geometry_and_valid_native_state() {
    let expected = FcstdCodec
        .decode(&mut Cursor::new(GEOMETRY), &DecodeOptions::default())
        .unwrap();
    for gui in [
        b"<Document SchemaVersion=\"1\"><ViewProviderData Count=\"0\"/></Document>".as_slice(),
        b"<Document SchemaVersion=\"1\"><Camera settings=\"\"/><Camera settings=\"\"/></Document>",
        b"<Document SchemaVersion=\"99\"><ViewProviderData Count=\"2\"/></Document>",
        b"<Document SchemaVersion=\"1\"><Camera/></Document>",
        b"<Document",
        b"\xff",
    ] {
        let source = rewrite_entry(GEOMETRY, "GuiDocument.xml", |_| gui.to_vec());
        let recovered = FcstdCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .unwrap();
        // Presentation is owned by GUI metadata; geometric entities are independent.
        assert_eq!(recovered.ir().model.faces, expected.ir().model.faces);
        let without_style = |bodies: &[cadmpeg_ir::topology::Body]| {
            let mut bodies = bodies.to_vec();
            for body in &mut bodies {
                body.visible = None;
                body.color = None;
            }
            bodies
        };
        assert_eq!(
            without_style(&recovered.ir().model.bodies),
            without_style(&expected.ir().model.bodies)
        );
        assert_eq!(recovered.ir().model.surfaces, expected.ir().model.surfaces);
        assert_valid_document(recovered.ir());
        assert!(crate::test_support::validate_native(recovered.ir()).is_empty());
        let documents = recovered
            .ir()
            .native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<GuiDocumentRecord>("gui_documents")
            .unwrap();
        assert_eq!(documents.len(), 1);
        let unresolved = recovered
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "source.gui-metadata-unresolved")
            .count();
        // An explicitly empty schema-1 provider table is a complete GUI fact.
        if gui == b"<Document SchemaVersion=\"1\"><ViewProviderData Count=\"0\"/></Document>" {
            assert_eq!(unresolved, 0);
            assert!(documents[0].states.is_empty());
            assert!(recovered.ir().model.view_presentations.is_empty());
        } else {
            assert_eq!(unresolved, 1, "{:?}", recovered.report().losses);
        }
    }
}

#[test]
fn missing_gui_material_member_keeps_visibility_and_gives_up_only_the_value() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4'><Objects Count='1'><Object name='Thing' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Thing'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Thing'><Properties Count='2'><Property name='Visibility' type='App::PropertyBool'><Bool value='true'/></Property><Property name='ShapeAppearance' type='App::PropertyMaterialList'><MaterialList file='missing.bin' version='3'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("GuiDocument.xml", gui),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    let visibility = properties
        .iter()
        .find(|property| property.name == "Visibility")
        .unwrap();
    assert_eq!(visibility.values[0].attributes["value"], "true");
    let material = properties
        .iter()
        .find(|property| property.name == "ShapeAppearance")
        .unwrap();
    assert!(material.values.is_empty());
    assert!(material.side_entries.is_empty());
    assert!(decoded
        .ir()
        .model
        .view_presentations
        .iter()
        .any(|view| view.visible == Some(true)));
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    let [(stub, retained)] = parts.as_slice() else {
        panic!("one missing GUI property value")
    };
    assert_eq!(
        retained.data(),
        Some("<MaterialList file='missing.bin' version='3'/>".as_bytes())
    );
    let unknowns = namespace
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    assert!(unknowns
        .iter()
        .find(|unknown| unknown.id.as_str() == *stub)
        .unwrap()
        .links
        .iter()
        .any(|id| id.as_str() == material.id));
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
}

#[test]
fn material_list_without_shape_keeps_each_appearance_without_bindings() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4'><Objects Count='1'><Object name='Thing' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Thing'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Thing'><Properties Count='1'><Property name='ShapeAppearance' type='App::PropertyMaterialList'><MaterialList file='materials.bin' version='2'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut materials = 2_u32.to_le_bytes().to_vec();
    for color in [0xff00_00ff_u32, 0x00ff_00ff] {
        for component in [0_u32, color, 0, 0] {
            materials.extend(component.to_le_bytes());
        }
        materials.extend(0.0_f32.to_le_bytes());
        materials.extend(0.0_f32.to_le_bytes());
    }
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("GuiDocument.xml", gui),
        RawEntry::stored("materials.bin", &materials),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let appearances = decoded
        .ir()
        .model
        .appearances
        .iter()
        .filter(|appearance| appearance.schema.as_deref() == Some("FCStd ShapeAppearance"))
        .collect::<Vec<_>>();
    assert_eq!(appearances.len(), 2);
    assert!(decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .all(|binding| appearances
            .iter()
            .all(|appearance| binding.appearance != appearance.id)));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "appearance.topology-color-count-mismatch"));
}

#[test]
fn missing_archive_tail_reports_unavailable_body_style() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Thing' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Thing'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let shape = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nVe 0.001 0 0 0 0 0 1001000 *\n+1 0 *";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Thing'><Properties Count='2'><Property name='Visibility' type='App::PropertyBool'><Bool value='false'/></Property><Property name='ShapeColor' type='App::PropertyColor'><PropertyColor value='4278190335'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("shape.brp", shape),
        RawEntry::stored("GuiDocument.xml", gui),
    ]);
    let full = FcstdCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(full.ir().model.bodies[0].visible, Some(false));
    assert_eq!(
        full.ir().model.bodies[0].color,
        Some(cadmpeg_ir::topology::Color::from_rgba8(255, 0, 0, 255))
    );
    let name = source
        .windows(b"GuiDocument.xml".len())
        .position(|window| window == b"GuiDocument.xml")
        .unwrap();
    let prefix = &source[..name - 30];
    let cut = FcstdCodec
        .decode(&mut Cursor::new(prefix), &DecodeOptions::default())
        .unwrap();
    let body = &cut.ir().model.bodies[0];
    assert!(body.visible.is_none());
    assert!(body.color.is_none());
    assert!(cut
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .any(|loss| loss
            .dropped_fields
            .iter()
            .any(|field| field.record == body.id.as_str() && field.field == "visible")));
    assert!(cut
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "record.unreadable")
        .any(|loss| loss
            .dropped_fields
            .iter()
            .any(|field| field.record == body.id.as_str() && field.field == "color")));
}

#[test]
fn failed_gui_member_does_not_add_design_scope() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4'><Objects Count='1'><Object name='Page' type='TechDraw::DrawPage'/></Objects><ObjectData Count='1'><Object name='Page'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Page'><Properties Count='1'><Property name='ShapeAppearance' type='App::PropertyMaterialList'><MaterialList file='materials.bin' version='2'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("GuiDocument.xml", gui),
        RawEntry::compressed("materials.bin", 12, b"bad", b"bad"),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    // #209 §1.1: Full decode reports requested design scope as delivered.
    assert!(decoded.ir().scope.design);
    assert!(!decoded
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::ArenaClass::Design));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"));
}

#[test]
fn shape_color_and_single_shape_material_keep_separate_appearances() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4' FileVersion='1'><Objects Count='1'><Object name='Thing' type='Part::Feature'/></Objects><ObjectData Count='1'><Object name='Thing'><Properties Count='1'><Property name='Shape' type='Part::PropertyPartShape'><Part file='shape.brp'/></Property></Properties></Object></ObjectData></Document>";
    let shape = b"CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 1\nVe 0.001 0 0 0 0 0 1001000 *\n+1 0 *";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Thing'><Properties Count='3'><Property name='ShapeColor' type='App::PropertyColor'><PropertyColor value='4278190335'/></Property><Property name='ShapeMaterial' type='App::PropertyMaterial'><PropertyMaterial ambientColor='1' diffuseColor='65535' specularColor='2' emissiveColor='3' shininess='0.25' transparency='0'/></Property><Property name='ShapeAppearance' type='App::PropertyMaterialList'><MaterialList file='materials.bin' version='2'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let mut material = 1_u32.to_le_bytes().to_vec();
    for component in [0_u32, 0x00ff_00ff, 0, 0] {
        material.extend(component.to_le_bytes());
    }
    material.extend(0.0_f32.to_le_bytes());
    material.extend(0.0_f32.to_le_bytes());
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("shape.brp", shape),
        RawEntry::stored("GuiDocument.xml", gui),
        RawEntry::stored("materials.bin", &material),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let body = &decoded.ir().model.bodies[0];
    let body_color = body.color.unwrap();
    assert!(body_color.g() > body_color.r());
    let bindings: Vec<_> = decoded
        .ir()
        .model
        .appearance_bindings
        .iter()
        .filter(|binding| {
            binding.target == cadmpeg_ir::appearance::AppearanceTarget::Body(body.id.clone())
        })
        .collect();
    assert_eq!(bindings.len(), 2);
    let appearances: Vec<_> = bindings
        .iter()
        .map(|binding| {
            decoded
                .ir()
                .model
                .appearances
                .iter()
                .find(|appearance| appearance.id == binding.appearance)
                .unwrap()
        })
        .collect();
    assert!(appearances
        .iter()
        .any(|appearance| appearance.schema.as_deref() == Some("FCStd ShapeAppearance")));
    assert!(
        appearances
            .iter()
            .any(|appearance| appearance.schema.as_deref()
                == Some("FCStd ViewProvider ShapeMaterial"))
    );
    let object = appearances
        .iter()
        .find(|appearance| appearance.schema.as_deref() == Some("FCStd ViewProvider ShapeMaterial"))
        .unwrap();
    let color = object.base_color.unwrap();
    assert!(color.b() > color.r());
    assert_eq!(object.properties["shininess"].get(), 0.25);
    assert!(object.properties.contains_key("diffuse_packed"));
    let effective = decoded
        .ir()
        .effective_display_color(&cadmpeg_ir::appearance::AppearanceTarget::Body(
            body.id.clone(),
        ))
        .unwrap();
    assert!(effective.g() > effective.r());
}

#[test]
fn malformed_color_member_gives_up_only_its_gui_value() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4'><Objects Count='1'><Object name='Thing' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Thing'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Thing'><Properties Count='2'><Property name='Visibility' type='App::PropertyBool'><Bool value='true'/></Property><Property name='DiffuseColor' type='App::PropertyColorList'><ColorList file='colors.bin'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let damaged = [2_u8, 0, 0, 0, 1, 2, 3];
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("GuiDocument.xml", gui),
        RawEntry::stored("colors.bin", &damaged),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    let color = properties
        .iter()
        .find(|property| property.name == "DiffuseColor")
        .unwrap();
    assert!(color.values.is_empty());
    assert!(color.side_entries.is_empty());
    assert!(decoded
        .ir()
        .model
        .view_presentations
        .iter()
        .any(|view| view.visible == Some(true)));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"
            && loss.message.contains("freecad.gui_property_value")));
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "source.gui-metadata-unresolved"));
    let parts = crate::test_support::source_fidelity::given_up(decoded.source_fidelity());
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].1.data(), Some(damaged.as_slice()));
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    assert!(entries
        .iter()
        .find(|entry| entry.name() == "colors.bin")
        .unwrap()
        .payload()
        .is_none());
    assert!(!decoded
        .source_fidelity()
        .unrepresented()
        .contains(&cadmpeg_ir::ArenaClass::Design));
}

#[test]
fn gui_property_without_value_keeps_its_header_and_sibling() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4'><Objects Count='1'><Object name='Thing' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Thing'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Thing'><Properties Count='2'><Property name='Visibility' type='App::PropertyBool'/><Property name='DisplayMode' type='App::PropertyEnumeration'><Integer value='1'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("GuiDocument.xml", gui),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    let bad = properties
        .iter()
        .find(|property| property.name == "Visibility")
        .unwrap();
    assert!(bad.values.is_empty());
    assert_eq!(bad.type_name, "App::PropertyBool");
    let view = decoded.ir().model.view_presentations.first().unwrap();
    assert!(view.visible.is_none());
    assert_eq!(view.display_mode.as_deref(), Some("1"));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"
            && loss.message.contains("freecad.gui_property_value")));
}

#[test]
fn gui_property_with_no_type_does_not_discard_its_siblings() {
    use cadmpeg_test_support::zip_fixture::{self, RawEntry};
    let document = b"<Document SchemaVersion='4'><Objects Count='1'><Object name='Thing' type='App::Feature'/></Objects><ObjectData Count='1'><Object name='Thing'><Properties Count='0'/></Object></ObjectData></Document>";
    let gui = b"<Document SchemaVersion='1'><ViewProviderData Count='1'><ViewProvider name='Thing'><Properties Count='2'><Property name='Broken'><String value='x'/></Property><Property name='Visibility' type='App::PropertyBool'><Bool value='true'/></Property></Properties></ViewProvider></ViewProviderData></Document>";
    let source = zip_fixture::archive(&[
        RawEntry::stored("Document.xml", document),
        RawEntry::stored("GuiDocument.xml", gui),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .unwrap();
    assert_eq!(properties.len(), 1);
    assert_eq!(properties[0].name, "Visibility");
    assert!(decoded
        .ir()
        .model
        .view_presentations
        .iter()
        .any(|view| view.visible == Some(true)));
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "record.unreadable"
            && loss.message.contains("freecad.gui_property")));
    assert!(crate::test_support::validate_native(decoded.ir()).is_empty());
}
