// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_solid_with_a_non_shell_child_is_a_general_body() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 3
1 1 0 0 10 0 1 0 0 0 0 1 0
1 -2 0 0 0 0 2 0 5 0 0 2 0
1 1 0 0 20 0 1 0 0 0 0 1 0
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
TShapes 8
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +8 0 -7 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +7 0 -8 0 *
Wi 1001000 +6 0 +5 0 *
Fa 0 0.001 1 0 1001000 +4 0 *
Sh 1001000 +3 2 *
So 1001000 +2 0 +6 0 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("solid with an edge child");
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
}
