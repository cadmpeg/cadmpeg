// SPDX-License-Identifier: Apache-2.0
//! Native registry conformance (#231 W3): the declared dispositions cover the
//! decoder's arena registry and state what the writer emits.

use crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
use crate::FcstdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

fn dispositions(ir: &cadmpeg_ir::CadIr) -> cadmpeg_ir::report::export::ExportReport {
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(ir, None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .expect("seed synthesizes");
    let mut written = Vec::new();
    plan.write_to(&mut written).expect("seed bytes write")
}

#[test]
fn native_registry_equals_dispositions() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("seed decodes");
    let declared: std::collections::BTreeSet<_> = dispositions(decoded.ir())
        .native_dispositions
        .namespaces
        .get("fcstd")
        .expect("fcstd namespace is declared")
        .arenas
        .keys()
        .cloned()
        .collect();
    let registry: std::collections::BTreeSet<_> = crate::native::ARENAS
        .iter()
        .map(|arena| (*arena).to_owned())
        .collect();
    assert_eq!(declared, registry);
}

#[test]
fn secondary_shape_payload_keeps_its_carrier_census() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="App::FeaturePython" name="Owner"/></Objects><ObjectData Count="1"><Object name="Owner"><Properties Count="1"><Property name="BackupShape" type="Part::PropertyPartShape"><Part file="Backup.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 1\nFa 0 0.001 1 0 1001000 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Backup.brp", brep),
    ]);
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let namespace = decoded.ir().native.namespace("fcstd").unwrap();
    let census = namespace
        .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
        .unwrap();
    assert_eq!(census.len(), 1);
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).unwrap();
    assert_eq!(
        report
            .native_dispositions
            .record_disposition("fcstd", "carrier_census", &census[0].id,),
        cadmpeg_ir::codec::write::NativeDisposition::Regenerated
    );
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let output_namespace = output.ir().native.namespace("fcstd").unwrap();
    let properties = output_namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(properties
        .iter()
        .any(|property| property.name == "BackupShape"));
    assert_eq!(
        output_namespace
            .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn primary_and_secondary_shapes_keep_separate_properties() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Owner"/></Objects><ObjectData Count="1"><Object name="Owner"><Properties Count="2"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Primary.brp"/></Property><Property name="BackupShape" type="Part::PropertyPartShape"><Part file="Backup.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 1\nFa 0 0.001 1 0 1001000 *\n+1 0 *";
    let backup = std::str::from_utf8(brep)
        .unwrap()
        .replace("1 0 0 0 0 0 1 1 0 0 0 1 0", "1 7 0 0 0 0 1 1 0 0 0 1 0");
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Primary.brp", brep),
        ("Backup.brp", backup.as_bytes()),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(input.ir().model.bodies.len(), 2);
    let plan = FcstdCodec
        .plan(
            EncodeInput::new(input.ir(), None),
            TargetRequest::Explicit("fcstd:schema-4"),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(output.ir().model.bodies.len(), 2);
    for surface in &input.ir().model.surfaces {
        assert!(output
            .ir()
            .model
            .surfaces
            .iter()
            .any(|other| other.geometry == surface.geometry));
    }
    let namespace = output.ir().native.namespace("fcstd").unwrap();
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(properties.iter().any(|property| property.name == "Shape"));
    assert!(properties
        .iter()
        .any(|property| property.name == "BackupShape"));
    assert_eq!(
        namespace
            .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
            .unwrap()
            .len(),
        2
    );
}
