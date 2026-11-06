// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn body_color_and_material_color_are_read_from_their_own_properties() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 3\nFa 0 0.001 0 0 2 1 1001000 *\nSh 1001000 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Shape"><Properties Count="2"><Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="4278190335"/></Property><Property name="ShapeMaterial" type="App::PropertyMaterial"><PropertyMaterial ambientColor="0" diffuseColor="65535" specularColor="0" emissiveColor="0" shininess="0" transparency="0"/></Property></Properties></ViewProvider></ViewProviderData><Camera settings=""/></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document),
                ("Shape.brp", brep),
                ("GuiDocument.xml", gui),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let red = cadmpeg_ir::topology::Color::from_rgba8(255, 0, 0, 255);
    let blue = cadmpeg_ir::topology::Color::from_rgba8(0, 0, 255, 255);
    assert_eq!(result.ir().model.bodies[0].color, Some(red));
    assert!(result
        .ir()
        .model
        .appearances
        .iter()
        .any(|appearance| appearance.base_color == Some(blue)));
}
