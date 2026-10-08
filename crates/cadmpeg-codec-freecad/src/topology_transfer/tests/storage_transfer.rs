// SPDX-License-Identifier: Apache-2.0
//! Surviving topology storage reaches retained admission through its owner.

use super::{assert_codec_retained_refusal, archive_entries};
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

fn archive(brep: &[u8]) -> Vec<u8> {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    archive_entries(&[("Document.xml", document), ("Shape.brp", brep)])
}

fn edge_archive() -> Vec<u8> {
    archive(b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 3
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +3 0 -2 0 *
+1 0 *")
}

#[test]
fn region_storage_transfer_refuses_and_preserves_identity() {
    let bytes = edge_archive();
    assert_codec_retained_refusal(&bytes, "FreeCAD region identity");
    let result = FcstdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    let region = &result.ir().model.regions[0];
    // Reverse wire reference1 resolves to subshape-first ordinal3.
    assert!(region.id.as_str().ends_with(":3"));
    assert_eq!(region.body, result.ir().model.bodies[0].id);
    assert_eq!(region.shells, vec![result.ir().model.shells[0].id.clone()]);
}

#[test]
fn edge_shell_storage_transfer_refuses_and_preserves_identity() {
    let bytes = edge_archive();
    assert_codec_retained_refusal(&bytes, "FreeCAD shell identity");
    let result = FcstdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    let shell = &result.ir().model.shells[0];
    // Reverse wire reference1 resolves to subshape-first ordinal3.
    assert!(shell.id.as_str().ends_with(":3"));
    assert_eq!(shell.wire_edges(), &[result.ir().model.edges[0].id.clone()]);
    assert!(shell.free_vertices().is_empty());
}

#[test]
fn vertex_shell_storage_transfer_refuses_and_preserves_identity() {
    let bytes = archive(b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 1
Ve 0.001 1 2 3 0 0 1001000 *
+1 0 *");
    assert_codec_retained_refusal(&bytes, "FreeCAD shell identity");
    let result = FcstdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    let shell = &result.ir().model.shells[0];
    assert!(shell.id.as_str().ends_with(":1"));
    assert_eq!(shell.free_vertices(), &[result.ir().model.vertices[0].id.clone()]);
    assert!(shell.wire_edges().is_empty());
    assert_eq!(result.ir().model.points[0].position().get(), cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0));
}

#[test]
fn pcurve_storage_transfer_refuses_and_preserves_identity() {
    let bytes = archive(b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 1
1 0 0 1 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 8
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +8 0 -7 0 *
Wi 1001000 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *");
    assert_codec_retained_refusal(&bytes, "FreeCAD pcurve candidate");
    let result = FcstdCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir().model.pcurves.len(), 1);
    // Third TShape, second representation, first member; colons are encoded.
    assert!(result.ir().model.pcurves[0].id.as_str().ends_with("3%3A2%3A1"));
    assert_eq!(result.ir().model.coedges[0].pcurves[0].pcurve, result.ir().model.pcurves[0].id);
}
