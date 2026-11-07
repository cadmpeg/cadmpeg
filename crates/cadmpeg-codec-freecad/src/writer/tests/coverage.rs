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
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
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
        .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
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
    let input_native = input.ir().native.namespace("fcstd").unwrap();
    let input_payloads = input_native
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let output_payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    for payload in &input_payloads {
        assert!(output_payloads.iter().any(|other| other.id == payload.id));
    }
    let input_census = input_native
        .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
        .unwrap();
    let output_census = namespace
        .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
        .unwrap();
    for census in &input_census {
        assert!(output_census.iter().any(|other| other.id == census.id));
    }
    for body in &input.ir().model.bodies {
        let peer = output
            .ir()
            .model
            .bodies
            .iter()
            .find(|other| other.id == body.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::to_value(peer).unwrap()
        );
    }
    for arena in ["applications", "document", "objects", "shape_payloads"] {
        let before = input_native.arenas().get(arena).unwrap();
        let after = namespace.arenas().get(arena).unwrap();
        for record in before {
            let peer = after
                .iter()
                .find(|other| other.id() == record.id())
                .unwrap();
            assert_eq!(
                serde_json::to_value(record).unwrap(),
                serde_json::to_value(peer).unwrap(),
                "{arena}"
            );
        }
    }
    let findings =
        cadmpeg_test_support::writer_closure::without_fidelity(&input, &FcstdCodec, &|bytes| {
            let result = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .map_err(|error| error.to_string())?;
            Ok(cadmpeg_test_support::writer_closure::Redecoded {
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: result
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        });
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn dimension_drawing_and_annotation_round_trip_together() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="TechDraw::DrawViewDimension" name="Dimension"/></Objects><ObjectData Count="1"><Object name="Dimension"><Properties Count="2"><Property name="FormatSpec" type="App::PropertyString"><String value="%0.2f"/></Property><Property name="Caption" type="App::PropertyString"><String value="Width"/></Property></Properties></Object></ObjectData></Document>"#;
    let input = FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::archive(
                std::str::from_utf8(xml).unwrap(),
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let findings =
        cadmpeg_test_support::writer_closure::without_fidelity(&input, &FcstdCodec, &|bytes| {
            let result = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .map_err(|error| error.to_string())?;
            Ok(cadmpeg_test_support::writer_closure::Redecoded {
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: result
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        });
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn primitive_feature_and_shape_round_trip_together() {
    let xml = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Box" name="Box" id="42"/></Objects><ObjectData Count="1"><Object name="Box"><Properties Count="5"><Property name="Label" type="App::PropertyString"><String value="Box"/></Property><Property name="Length" type="App::PropertyLength"><Float value="3"/></Property><Property name="Width" type="App::PropertyLength"><Float value="4"/></Property><Property name="Height" type="App::PropertyLength"><Float value="5"/></Property><Property name="Shape" type="Part::PropertyPartShape" status="128"><Part file="Box.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 1\nFa 0 0.001 1 0 1001000 *\n+1 0 *";
    let source = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", xml),
        ("Box.brp", brep),
    ]);
    let input = FcstdCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let source = input
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let written = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    for record in &source {
        let peer = written.iter().find(|other| other.id == record.id).unwrap();
        assert_eq!(
            serde_json::to_value(record).unwrap(),
            serde_json::to_value(peer).unwrap(),
            "{}",
            record.name
        );
    }
    let findings =
        cadmpeg_test_support::writer_closure::without_fidelity(&input, &FcstdCodec, &|bytes| {
            let result = FcstdCodec
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .map_err(|error| error.to_string())?;
            Ok(cadmpeg_test_support::writer_closure::Redecoded {
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: result
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
                members: Vec::new(),
            })
        });
    assert!(findings.is_empty(), "{findings:#?}");
    let mut edited = input.ir().clone();
    let height = edited
        .model
        .parameters
        .iter_mut()
        .find(|parameter| parameter.name == "Height")
        .unwrap();
    let length = cadmpeg_ir::scalar::Length::new(7.0).unwrap();
    height.value = Some(cadmpeg_ir::features::ParameterValue::Length(length));
    height.expression = cadmpeg_ir::features::Expression::text("7 mm");
    let plan = FcstdCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let properties = output
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let height = properties
        .iter()
        .find(|property| property.name == "Height")
        .unwrap();
    assert!(height.dynamic.is_none());
    assert_eq!(height.values()[0].attributes["value"], "7");
}

#[test]
fn forward_dependency_keeps_source_declaration_order() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="2" Dependencies="1"><ObjectDeps Name="Consumer" Count="1"><Dep Name="Driver"/></ObjectDeps><ObjectDeps Name="Driver" Count="0"/><Object name="Consumer" type="PartDesign::Feature"/><Object name="Driver" type="PartDesign::Feature"/></Objects><ObjectData Count="2"><Object name="Consumer"><Properties Count="0"/></Object><Object name="Driver"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let bytes = crate::test_support::test_archive::archive(document);
    let input = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    for arena in ["objects", "applications"] {
        let records = |ir: &cadmpeg_ir::CadIr| {
            ir.native
                .namespace("fcstd")
                .unwrap()
                .arenas()
                .get(arena)
                .unwrap()
                .iter()
                .map(|record| serde_json::to_value(record).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(records(input.ir()), records(output.ir()), "{arena}");
    }
    let rank = |ir: &cadmpeg_ir::CadIr, name: &str| {
        ir.model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap()
            .ordinal
    };
    assert!(rank(output.ir(), "Driver") < rank(output.ir(), "Consumer"));
}

#[test]
fn datum_placement_keeps_native_property_metadata() {
    use cadmpeg_ir::features::{FeatureDatumPlaneFrame, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};

    let document = r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="PartDesign::Plane" name="Plane" id="1"/></Objects><ObjectData Count="1"><Object name="Plane"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement" status="128"><PropertyPlacement Px="1.0" Py="2.0" Pz="3.0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object></ObjectData></Document>"#;
    let bytes = crate::test_support::test_archive::archive(document);
    let input = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let plan = FcstdCodec
        .plan(EncodeInput::new(input.ir(), None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let placement = |ir: &cadmpeg_ir::CadIr| {
        ir.native
            .namespace("fcstd")
            .unwrap()
            .arena_as::<crate::native::PropertyRecord>("properties")
            .unwrap()
            .into_iter()
            .find(|property| property.name == "Placement")
            .unwrap()
    };
    assert_eq!(
        serde_json::to_value(placement(input.ir())).unwrap(),
        serde_json::to_value(placement(output.ir())).unwrap()
    );
    let frame = FeatureDatumPlaneFrame::new(
        Point3::new(9.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .unwrap();
    let mut edited = input.ir().clone();
    edited.model.features[0]
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::DatumPlane {
            frame,
        }));
    let plan = FcstdCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(placement(output.ir()).values()[0].attributes["Px"], "9");
}
