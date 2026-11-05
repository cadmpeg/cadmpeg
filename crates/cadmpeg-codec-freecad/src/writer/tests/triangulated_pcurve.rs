// SPDX-License-Identifier: Apache-2.0
//! Parameter curves on faces with triangulations.

use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn face_triangulation_keeps_its_edge_parameter_curve() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 1\n1 0 0 1 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 1\n2 1 2 p 0.01 1 0 1\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 1\n3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 7\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 6 1 1 0 2 1 1 0 0 1 0 1001000 +7 0 -6 0 *\nWi 1001000 +5 0 *\nFa 0 0.001 1 0 2 1 1001000 +4 0 *\nSh 1001000 +3 0 *\nSo 1001000 +2 0 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Shape.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.pcurves.len(), 1);
    assert_eq!(input.ir().model.coedges[0].pcurves.len(), 1);
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.tessellations.len(), 1);
    assert_eq!(output.ir().model.coedges[0].pcurves.len(), 1);
    assert_eq!(
        output.ir().model.pcurves[0].geometry,
        input.ir().model.pcurves[0].geometry
    );
}
