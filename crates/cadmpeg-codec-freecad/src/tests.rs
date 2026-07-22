use std::io::{Cursor, Write};

use cadmpeg_ir::{Codec, Confidence, DecodeOptions, Encoder};
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

const CORE_DESIGN_PRODUCT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
));

#[test]
fn writes_typed_property_edits_and_preserves_other_entries() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let source_entries = decoded
        .ir
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let mut edited = decoded.ir.clone();
    FcstdCodec
        .set_property_value_attribute(
            &mut edited,
            crate::FcstdPropertyOwner::Document,
            "Label",
            0,
            "value",
            "edited & verified",
        )
        .expect("edit Label");

    let mut encoded = Vec::new();
    let report = FcstdCodec
        .encode(&edited, &mut encoded)
        .expect("encode edit");
    assert!(report.losses.is_empty());
    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode output");
    let output_namespace = round_trip.ir.native.namespace("fcstd").expect("namespace");
    let output_properties = output_namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let output_label = output_properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("document", "0") && property.name == "Label"
        })
        .expect("document Label");
    assert_eq!(
        output_label.values[0]
            .attributes
            .get("value")
            .map(String::as_str),
        Some("edited & verified")
    );
    let output_entries = output_namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    for source in source_entries
        .iter()
        .filter(|entry| entry.name != "Document.xml")
    {
        let output = output_entries
            .iter()
            .find(|entry| entry.name == source.name)
            .expect("preserved entry");
        assert_eq!(output.data, source.data, "{}", source.name);
    }
    assert!(crate::validate_native(&round_trip.ir).is_empty());
}

#[test]
fn write_target_and_source_requirements_are_explicit() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");
    let unsupported = FcstdCodec
        .encode_with_options(
            &decoded.ir,
            &mut Vec::new(),
            crate::FcstdWriteOptions {
                schema_version: 3,
                file_version: 1,
            },
        )
        .expect_err("unsupported target must fail");
    assert!(unsupported.to_string().contains("SchemaVersion=3"));

    let source_less = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let missing_graph = FcstdCodec
        .encode(&source_less, &mut Vec::new())
        .expect_err("missing graph must fail");
    assert!(missing_graph.to_string().contains("source-less"));
}

#[test]
fn writer_rejects_unserialized_declaration_and_stale_payload_edits() {
    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode source");

    let mut declaration_edit = decoded.ir.clone();
    let namespace = declaration_edit.native.namespace_mut("fcstd");
    let mut objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    objects[0].type_name = "App::FeaturePython".into();
    namespace
        .set_arena("objects", &objects)
        .expect("replace objects");
    let error = FcstdCodec
        .encode(&declaration_edit, &mut Vec::new())
        .expect_err("unserialized declaration edit must fail");
    assert!(error.to_string().contains("declaration edits"));

    let mut stale_entry = decoded.ir;
    let namespace = stale_entry.native.namespace_mut("fcstd");
    let mut entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    entries
        .iter_mut()
        .find(|entry| entry.name != "Document.xml")
        .expect("side entry")
        .data
        .push(0);
    namespace
        .set_arena("entries", &entries)
        .expect("replace entries");
    let error = FcstdCodec
        .encode(&stale_entry, &mut Vec::new())
        .expect_err("stale entry metadata must fail");
    assert!(error.to_string().contains("stale length or digest"));
}

#[test]
fn builds_and_writes_a_source_less_typed_application_graph() {
    let mut builder = crate::FcstdDocumentBuilder::new("source-less & portable");
    builder
        .add_object("Box", "Part::Box")
        .expect("add object")
        .add_property(
            "Box",
            "Label",
            "App::PropertyString",
            vec![crate::FcstdPropertyValue::attribute(
                "String",
                "value",
                "Generated Box",
            )],
        )
        .expect("add label")
        .add_property(
            "Box",
            "Length",
            "App::PropertyLength",
            vec![crate::FcstdPropertyValue::attribute(
                "Float", "value", "12.5",
            )],
        )
        .expect("add length")
        .add_property(
            "Box",
            "Width",
            "App::PropertyLength",
            vec![crate::FcstdPropertyValue::attribute("Float", "value", "7")],
        )
        .expect("add width")
        .add_property(
            "Box",
            "Height",
            "App::PropertyLength",
            vec![crate::FcstdPropertyValue::attribute("Float", "value", "3")],
        )
        .expect("add height")
        .add_object("Part", "App::Part")
        .expect("add part")
        .add_dependency("Part", "Box")
        .expect("add dependency")
        .add_property(
            "Part",
            "Group",
            "App::PropertyLinkList",
            vec![crate::FcstdPropertyValue::empty("LinkList")
                .with_attribute("count", "1")
                .with_child(crate::FcstdPropertyValue::attribute("Link", "value", "Box"))],
        )
        .expect("add group")
        .add_side_entry("Payload.bin", b"extension payload".to_vec())
        .expect("add payload");
    let mut ir = builder.build().expect("build source-less graph");
    assert!(crate::validate_native(&ir).is_empty());
    FcstdCodec
        .replace_side_entry(&mut ir, "Payload.bin", b"edited payload".to_vec())
        .expect("replace side entry");

    let mut encoded = Vec::new();
    FcstdCodec.encode(&ir, &mut encoded).expect("write graph");
    let round_trip = FcstdCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode generated file");
    let namespace = round_trip.ir.native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0].name, "Box");
    assert_eq!(objects[0].type_name, "Part::Box");
    assert_eq!(objects[1].dependencies, vec![objects[0].id.clone()]);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.name == "Payload.bin")
            .map(|entry| entry.data.as_slice()),
        Some(b"edited payload".as_slice())
    );
}

fn assert_valid_document(ir: &cadmpeg_ir::CadIr) {
    let errors = cadmpeg_ir::validate(ir, Vec::new())
        .findings
        .into_iter()
        .filter(|finding| finding.severity >= cadmpeg_ir::Severity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn public_cc0_fixtures_decode_deterministically_without_blocking_loss() {
    let fixtures: [(&str, &[u8]); 11] = [
        (
            "external_component.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/external_component.FCStd"
            )),
        ),
        (
            "product_assembly.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/product_assembly.FCStd"
            )),
        ),
        (
            "core_operations.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/core_operations.FCStd"
            )),
        ),
        (
            "sketch_constraints.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/sketch_constraints.FCStd"
            )),
        ),
        (
            "gui_appearance.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/gui_appearance.FCStd"
            )),
        ),
        (
            "design_history.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/design_history.FCStd"
            )),
        ),
        (
            "binary_exact_shape.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/binary_exact_shape.FCStd"
            )),
        ),
        (
            "application_payloads.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/application_payloads.FCStd"
            )),
        ),
        (
            "geometry_topology.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/geometry_topology.FCStd"
            )),
        ),
        (
            "core_design_product.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
            )),
        ),
        (
            "techdraw_annotations.FCStd",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../corpus/freecad_fcstd/fixtures/techdraw_annotations.FCStd"
            )),
        ),
    ];
    for (name, bytes) in fixtures {
        let first = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let second = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            first.ir.to_canonical_json().expect("canonical fixture"),
            second.ir.to_canonical_json().expect("canonical fixture"),
            "{name} is nondeterministic"
        );
        assert!(
            first
                .report
                .losses
                .iter()
                .all(|loss| loss.severity < cadmpeg_ir::Severity::Blocking),
            "{name}: {:#?}",
            first.report.losses
        );
        let native_findings = crate::validate_native(&first.ir);
        assert!(native_findings.is_empty(), "{name}: {native_findings:#?}");
        assert_valid_document(&first.ir);
    }
}

#[test]
fn rejects_malformed_sketch_record_counts() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="2">
<Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let error = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect_err("count mismatch");
    assert!(error
        .to_string()
        .contains("declares 2 records but contains 1"));
}

#[test]
fn transfers_point_and_elliptical_sketch_geometry_without_fabricated_defaults() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="4">
 <Geometry type="Part::GeomPoint"><Point X="1" Y="2"/></Geometry>
 <Geometry type="Part::GeomEllipse"><Ellipse CenterX="3" CenterY="4" MajorRadius="6" MinorRadius="2" MajorAxisX="0" MajorAxisY="1"/></Geometry>
 <Geometry type="Part::GeomArcOfEllipse"><ArcOfEllipse CenterX="0" CenterY="0" MajorRadius="5" MinorRadius="3" MajorAngle="0.25" FirstParameter="0.5" LastParameter="1.5"/></Geometry>
 <Geometry type="Part::GeomCircle"><Circle CenterX="9" CenterY="9"/></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("sketch geometry");
    let entities = &result.ir.model.sketch_entities;
    assert!(matches!(
        entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Point { position }
            if position == cadmpeg_ir::math::Point2::new(1.0, 2.0)
    ));
    assert!(matches!(
        entities[1].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Ellipse {
            major_angle: cadmpeg_ir::features::Angle(angle),
            start_angle: None,
            end_angle: None,
            ..
        } if (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
    assert!(matches!(
        entities[2].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Ellipse {
            start_angle: Some(cadmpeg_ir::features::Angle(0.5)),
            end_angle: Some(cadmpeg_ir::features::Angle(1.5)),
            ..
        }
    ));
    assert!(matches!(
        entities[3].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Native { .. }
    ));
}

#[test]
fn transfers_full_and_bounded_sketch_conics() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Conics" id="1"/></Objects>
<ObjectData Count="1"><Object name="Conics"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="6">
 <Geometry type="Part::GeomHyperbola"><Hyperbola CenterX="1" CenterY="2" AngleXU="0.25" MajorRadius="5" MinorRadius="3"/></Geometry>
 <Geometry type="Part::GeomArcOfHyperbola"><ArcOfHyperbola CenterX="2" CenterY="3" AngleXU="0.5" MajorRadius="7" MinorRadius="4" StartAngle="-1" EndAngle="1.5"/></Geometry>
 <Geometry type="Part::GeomParabola"><Parabola CenterX="3" CenterY="4" AngleXU="0.75" Focal="2"/></Geometry>
 <Geometry type="Part::GeomArcOfParabola"><ArcOfParabola CenterX="4" CenterY="5" AngleXU="1" Focal="2.5" StartAngle="-2" EndAngle="3"/></Geometry>
 <Geometry type="Part::GeomArcOfCircle"><ArcOfCircle CenterX="0" CenterY="0" Radius="4" StartAngle="0.2" EndAngle="1.2"/></Geometry>
 <Geometry type="Part::GeomArcOfEllipse"><ArcOfEllipse CenterX="0" CenterY="1" AngleXU="0.3" MajorRadius="6" MinorRadius="2" StartAngle="0.4" EndAngle="1.4"/></Geometry>
</GeometryList></Property></Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("sketch conics");
    let entities = &result.ir.model.sketch_entities;
    assert_eq!(entities.len(), 6);
    assert!(matches!(
        entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Hyperbola {
            start_parameter: None,
            end_parameter: None,
            ..
        }
    ));
    assert!(matches!(
        entities[1].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Hyperbola {
            start_parameter: Some(-1.0),
            end_parameter: Some(1.5),
            ..
        }
    ));
    assert!(matches!(
        entities[2].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Parabola {
            focal_length: cadmpeg_ir::features::Length(2.0),
            start_parameter: None,
            ..
        }
    ));
    assert!(matches!(
        entities[3].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Parabola {
            focal_length: cadmpeg_ir::features::Length(2.5),
            start_parameter: Some(-2.0),
            end_parameter: Some(3.0),
            ..
        }
    ));
    assert!(matches!(
        entities[4].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Arc { .. }
    ));
    assert!(matches!(
        entities[5].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Ellipse {
            start_angle: Some(_),
            end_angle: Some(_),
            ..
        }
    ));
    assert!(entities.iter().all(|entity| !matches!(
        entity.geometry,
        cadmpeg_ir::sketches::SketchGeometry::Native { .. }
    )));
    assert!(result.report.losses.is_empty());
    assert_valid_document(&result.ir);
}

#[test]
fn transfers_bounded_rational_sketch_nurbs() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/></Objects>
<ObjectData Count="1"><Object name="Sketch"><Properties Count="1">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
 <Geometry type="Part::GeomBSplineCurve"><BSplineCurve PolesCount="3" KnotsCount="2" Degree="2" IsPeriodic="0">
  <Pole X="0" Y="0" Z="0" Weight="1"/>
  <Pole X="1" Y="2" Z="0" Weight="0.5"/>
  <Pole X="3" Y="0" Z="0" Weight="1"/>
  <Knot Value="0" Mult="3"/>
  <Knot Value="1" Mult="3"/>
 </BSplineCurve></Geometry>
</GeometryList></Property>
</Properties></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("sketch NURBS");
    assert!(matches!(
        &result.ir.model.sketch_entities[0].geometry,
        cadmpeg_ir::sketches::SketchGeometry::Nurbs {
            degree: 2,
            knots,
            control_points,
            weights: Some(weights),
            periodic: false,
        } if knots == &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
            && control_points.len() == 3
            && weights == &[1.0, 0.5, 1.0]
    ));
}

#[test]
fn neutralizes_symmetric_locus_distance_and_point_on_object_constraints() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2"><Object type="Sketcher::SketchObject" name="Sketch" id="1"/><Object type="Part::Feature" name="Source" id="2"/></Objects>
<ObjectData Count="2"><Object name="Sketch"><Properties Count="4">
<Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="3">
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="1" EndX="1" EndY="1"/></Geometry>
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0.5" StartY="-1" EndX="0.5" EndY="2"/></Geometry>
</GeometryList></Property>
<Property name="ExternalGeometry" type="App::PropertyLinkSubList"><LinkList count="1"><Link object="Source" sub="Edge1"/></LinkList></Property>
<Property name="ExternalGeo" type="Part::PropertyGeometryList"><GeometryList count="3">
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="1" EndY="0"/></Geometry>
 <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="0" EndY="1"/></Geometry>
 <Geometry type="Part::GeomCircle"><Circle CenterX="4" CenterY="5" Radius="2"/></Geometry>
</GeometryList></Property>
<Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="12">
 <Constrain Type="14" First="0" FirstPos="1" Second="1" SecondPos="1" Third="2" ThirdPos="0"/>
 <Constrain Type="6" First="0" FirstPos="1" Second="1" SecondPos="2" Value="4" IsDriving="1"/>
 <Constrain Name="OnAxis" MetaData="reviewed" Type="13" Orientation="4" Value="0" LabelDistance="2.5" LabelPosition="0.25" IsDriving="0" IsInVirtualSpace="1" IsVisible="0" IsActive="1" First="0" FirstPos="1" Second="2" SecondPos="0"/>
 <Constrain Type="16" First="0" FirstPos="2" Second="1" SecondPos="1" Third="2" ThirdPos="0" Value="1.33" IsDriving="1"/>
 <Constrain Type="19" First="0" FirstPos="0" Value="0.75" IsDriving="1"/>
 <Constrain Type="15" InternalAlignmentType="9" InternalAlignmentIndex="2" First="0" FirstPos="0" Second="1" SecondPos="0"/>
 <Constrain Type="20" ElementIds="2 0 1" ElementPositions="0 0 0"/>
 <Constrain Type="21" MetaData="{&quot;text&quot;:&quot;R42&quot;,&quot;font&quot;:&quot;Mono&quot;,&quot;isTextHeight&quot;:false}" ElementIds="2 0" ElementPositions="0 0"/>
 <Constrain Type="0" IsActive="0"/>
 <Constrain Type="13" First="0" FirstPos="1" Second="-1" SecondPos="0"/>
 <Constrain Type="6" First="-1" FirstPos="1" Second="0" SecondPos="1" Value="2" IsDriving="1"/>
 <Constrain Type="13" First="0" FirstPos="2" Second="-3" SecondPos="0"/>
</ConstraintList></Property>
</Properties></Object><Object name="Source"><Properties Count="0"/></Object></ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("sketch constraints");
    let constraint = |index: usize| {
        result
            .ir
            .model
            .sketch_constraints
            .iter()
            .find(|constraint| constraint.id.0.ends_with(&format!(":{index}")))
            .expect("constraint index")
    };
    assert!(matches!(
        constraint(1).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Symmetric { .. }
    ));
    let point_on_object = constraint(3);
    assert_eq!(point_on_object.name.as_deref(), Some("OnAxis"));
    assert_eq!(point_on_object.metadata.as_deref(), Some("reviewed"));
    assert_eq!(point_on_object.orientation, Some(4));
    assert_eq!(point_on_object.label_distance, Some(2.5));
    assert_eq!(point_on_object.label_position, Some(0.25));
    assert_eq!(point_on_object.driving, Some(false));
    assert_eq!(point_on_object.virtual_space, Some(true));
    assert_eq!(point_on_object.visible, Some(false));
    assert_eq!(point_on_object.active, Some(true));
    assert!(matches!(
        constraint(4).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::SnellsLaw { .. }
    ));
    assert!(matches!(
        constraint(5).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Weight { .. }
    ));
    assert!(matches!(
        result
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id.0.ends_with(":constraint:4"))
            .expect("Snell parameter")
            .value,
        Some(cadmpeg_ir::features::ParameterValue::Real(value)) if (value - 1.33).abs() < 1e-12
    ));
    assert!(matches!(
        result
            .ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.id.0.ends_with(":constraint:5"))
            .expect("weight parameter")
            .value,
        Some(cadmpeg_ir::features::ParameterValue::Real(value)) if (value - 0.75).abs() < 1e-12
    ));
    assert!(matches!(
        constraint(6).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::InternalAlignment {
            alignment: cadmpeg_ir::sketches::SketchInternalAlignment::BsplineControlPoint,
            index: Some(2),
            ..
        }
    ));
    assert!(matches!(
        constraint(7).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Group { ref elements }
            if elements.len() == 3
    ));
    assert!(matches!(
        constraint(8).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Text {
            ref text,
            font: Some(ref font),
            is_text_height: false,
            ..
        } if text == "R42" && font == "Mono"
    ));
    assert!(matches!(
        constraint(9).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::Disabled
    ));
    assert!(matches!(
        constraint(10).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::PointOnObject { .. }
    ));
    assert!(matches!(
        constraint(11).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::DistanceLoci { .. }
    ));
    assert!(matches!(
        constraint(12).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::PointOnObject { .. }
    ));
    assert!(result.ir.model.sketch_entities.iter().any(|entity| {
        entity.id.0.ends_with(":reference-horizontal-axis")
            && matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::ReferenceLine { .. }
            )
    }));
    assert!(result
        .ir
        .model
        .sketch_entities
        .iter()
        .any(|entity| entity.id.0.ends_with(":reference-root-point")));
    let external = result
        .ir
        .model
        .sketch_entities
        .iter()
        .find(|entity| entity.id.0.ends_with(":external:0"))
        .expect("external geometry");
    assert!(matches!(
        external.geometry,
        cadmpeg_ir::sketches::SketchGeometry::Circle { .. }
    ));
    assert!(external
        .geometry_ref
        .as_deref()
        .is_some_and(|reference| reference.ends_with(":ExternalGeometry")));
    assert_eq!(external.endpoint_refs, ["Edge1"]);
    assert!(matches!(
        constraint(2).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::DistanceLoci { .. }
    ));
    assert!(matches!(
        constraint(3).definition,
        cadmpeg_ir::sketches::SketchConstraintDefinition::PointOnObject {
            point: cadmpeg_ir::sketches::SketchLocus::Start(_),
            ..
        }
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_revolution_fillet_and_chamfer_semantics() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
 <Object type="PartDesign::Revolution" name="Revolution" id="2"/>
 <Object type="PartDesign::Fillet" name="Fillet" id="3"/>
 <Object type="PartDesign::Chamfer" name="Chamfer" id="4"/>
 <ObjectDeps Name="Revolution"><Dep Name="Sketch"/></ObjectDeps>
 <ObjectDeps Name="Fillet"><Dep Name="Revolution"/></ObjectDeps>
 <ObjectDeps Name="Chamfer"><Dep Name="Fillet"/></ObjectDeps>
</Objects>
<ObjectData Count="4">
 <Object name="Sketch"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Revolution"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="180"/></Property>
 </Properties></Object>
 <Object name="Fillet"><Properties Count="3">
  <Property name="Base" type="App::PropertyLinkSub"><Link object="Revolution" sub="Edge1"/></Property>
  <Property name="Radius" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="UseAllEdges" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Chamfer"><Properties Count="5">
  <Property name="Base" type="App::PropertyLinkSub"><Link object="Fillet" sub="Edge2"/></Property>
  <Property name="ChamferType" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Size" type="App::PropertyLength"><Float value="1.5"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="30"/></Property>
  <Property name="FlipDirection" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("core operations");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(matches!(
        definition("Revolution"),
        cadmpeg_ir::features::FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                profile: Some(cadmpeg_ir::features::ProfileRef::Sketch(_)),
                extent: Some(cadmpeg_ir::features::Extent::Angle { angle }),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join
        } if (angle.0 - std::f64::consts::PI).abs() < 1e-12
    ));
    assert!(matches!(
        definition("Fillet"),
        cadmpeg_ir::features::FeatureDefinition::Fillet {
            edges: cadmpeg_ir::features::EdgeSelection::All,
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(2.0)
            },
            ..
        }
    ));
    assert!(matches!(
        definition("Chamfer"),
        cadmpeg_ir::features::FeatureDefinition::Chamfer {
            spec: cadmpeg_ir::features::ChamferSpec::DistanceAngle {
                distance: cadmpeg_ir::features::Length(1.5),
                angle,
            },
            flip_direction: Some(true),
            ..
        } if (angle.0 - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
}

#[test]
fn transfers_non_default_revolution_branches() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
 <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
 <Object type="PartDesign::Revolution" name="ToFirst" id="2"/>
 <Object type="PartDesign::Revolution" name="ToFace" id="3"/>
 <Object type="PartDesign::Revolution" name="TwoAngles" id="4"/>
 <Object type="PartDesign::Revolution" name="Midplane" id="5"/>
 <Object type="PartDesign::Groove" name="ThroughAll" id="6"/>
 <Object type="Part::Revolution" name="Standalone" id="7"/>
</Objects>
<ObjectData Count="7">
 <Object name="Sketch"><Properties Count="0"/></Object>
 <Object name="ToFirst"><Properties Count="4">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="1" y="2" z="3"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="2" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="2"/></Property>
 </Properties></Object>
 <Object name="ToFace"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="UpToFace" type="App::PropertyLinkSub"><Link object="Standalone" sub="Face1"/></Property>
 </Properties></Object>
 <Object name="TwoAngles"><Properties Count="6">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="4"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="120"/></Property>
  <Property name="Angle2" type="App::PropertyAngle"><Float value="30"/></Property>
 </Properties></Object>
 <Object name="Midplane"><Properties Count="10">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="3" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="90"/></Property>
  <Property name="Midplane" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="ReferenceAxis" type="App::PropertyLinkSub"><Link object="Sketch" sub="H_Axis"/></Property>
  <Property name="FuseOrder" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="ThroughAll"><Properties Count="4">
  <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="Standalone"><Properties Count="8">
  <Property name="Source" type="App::PropertyLink"><Link value="Sketch"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="0" z="4"/></Property>
  <Property name="Angle" type="App::PropertyFloatConstraint"><Float value="45"/></Property>
  <Property name="Symmetric" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AxisLink" type="App::PropertyLinkSub"><Link object="Sketch" sub="Edge1"/></Property>
  <Property name="FaceMakerClass" type="App::PropertyString"><String value="Part::FaceMakerUnified"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("revolution branches");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    use cadmpeg_ir::features::{BooleanOp, Extent, FeatureDefinition};
    assert!(matches!(
        definition("ToFirst"),
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(axis),
                extent: Some(Extent::ToFirst),
                ..
            },
            ..
        } if axis.direction.y == 1.0 && axis.origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
    ));
    assert!(matches!(
        definition("ToFace"),
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(Extent::ToFace { .. }),
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        definition("TwoAngles"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { extent: Some(Extent::TwoSidedAngles { first, second }), .. }, .. }
            if (first.0 - 120_f64.to_radians()).abs() < 1e-12 && (second.0 - 30_f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        definition("Midplane"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { axis: Some(axis), extent: Some(Extent::SymmetricAngle { .. }), axis_reference: Some(cadmpeg_ir::features::PathRef::Native(reference)), fuse_order: Some(cadmpeg_ir::features::RevolutionFuseOrder::FeatureFirst), solid: Some(true), allow_multi_profile_faces: Some(false), .. }, .. }
            if axis.direction.y == -1.0 && reference.ends_with(":ReferenceAxis")
    ));
    assert!(matches!(
        definition("ThroughAll"),
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                extent: Some(Extent::ThroughAll),
                ..
            },
            op: BooleanOp::Cut
        }
    ));
    assert!(matches!(
        definition("Standalone"),
        FeatureDefinition::Revolve { construction: cadmpeg_ir::features::RevolutionConstruction { profile: Some(cadmpeg_ir::features::ProfileRef::Sketch(_)), axis: Some(axis), extent: Some(Extent::SymmetricAngle { .. }), axis_reference: Some(cadmpeg_ir::features::PathRef::Native(reference)), solid: Some(true), face_maker_class: Some(face_maker), .. }, op: BooleanOp::NewBody }
            if axis.direction.z == 1.0 && reference.ends_with(":AxisLink")
                && face_maker == "Part::FaceMakerUnified"
    ));
}

#[test]
fn transfers_part_and_partdesign_analytic_primitives() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Box" name="Box" id="1"/>
 <Object type="PartDesign::AdditiveCylinder" name="AddCylinder" id="2"/>
 <Object type="PartDesign::SubtractiveCone" name="CutCone" id="3"/>
 <ObjectDeps Name="AddCylinder"><Dep Name="Box"/></ObjectDeps>
 <ObjectDeps Name="CutCone"><Dep Name="AddCylinder"/></ObjectDeps>
</Objects>
<ObjectData Count="3">
 <Object name="Box"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="20"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="30"/></Property>
 </Properties></Object>
 <Object name="AddCylinder"><Properties Count="3">
  <Property name="Radius" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="180"/></Property>
 </Properties></Object>
 <Object name="CutCone"><Properties Count="4">
  <Property name="Radius1" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Radius2" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="6"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="360"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("primitives");
    assert_eq!(result.ir.ir_version, cadmpeg_ir::IR_VERSION);
    let feature = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("primitive")
            .definition
    };
    assert!(matches!(
        feature("Box"),
        cadmpeg_ir::features::FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::Box {
                length: cadmpeg_ir::features::Length(10.0),
                width: cadmpeg_ir::features::Length(20.0),
                height: cadmpeg_ir::features::Length(30.0),
            },
            op: cadmpeg_ir::features::BooleanOp::NewBody,
        }
    ));
    assert!(matches!(
        feature("AddCylinder"),
        cadmpeg_ir::features::FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::Cylinder {
                angle: cadmpeg_ir::features::Angle(angle),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
        } if (angle - std::f64::consts::PI).abs() < 1e-12
    ));
    assert!(matches!(
        feature("CutCone"),
        cadmpeg_ir::features::FeatureDefinition::Primitive {
            solid: cadmpeg_ir::features::PrimitiveSolid::Cone { .. },
            op: cadmpeg_ir::features::BooleanOp::Cut,
        }
    ));
    assert!(result.report.losses.is_empty());
    let findings = cadmpeg_ir::validate(&result.ir, Vec::new()).findings;
    assert!(
        findings
            .iter()
            .all(|finding| finding.check != cadmpeg_ir::Check::GeometricConsistency),
        "{findings:#?}"
    );
}

#[test]
fn transfers_parametric_part_helix_and_spiral_construction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Helix" name="Helix" id="1"/>
 <Object type="Part::Spiral" name="Spiral" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Helix"><Properties Count="7">
  <Property name="Pitch" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="20"/></Property>
  <Property name="Radius" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="12"/></Property>
  <Property name="SegmentLength" type="App::PropertyQuantity"><Float value="0.5"/></Property>
  <Property name="LocalCoord" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Style" type="App::PropertyEnumeration"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="Spiral"><Properties Count="4">
  <Property name="Growth" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Radius" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="Rotations" type="App::PropertyQuantity"><Float value="3.5"/></Property>
  <Property name="SegmentLength" type="App::PropertyQuantity"><Float value="0.25"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("parametric curves");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Helix"),
        cadmpeg_ir::features::FeatureDefinition::Helix {
            radius: cadmpeg_ir::features::Length(3.0),
            pitch: cadmpeg_ir::features::Length(4.0),
            revolutions: 5.0,
            clockwise: true,
            cone_angle: Some(cadmpeg_ir::features::Angle(angle)),
            segment_turns: Some(0.5),
            construction_style: Some(cadmpeg_ir::features::HelixConstructionStyle::Corrected),
            radial_growth: None,
            ..
        } if (*angle - 12_f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        definition("Spiral"),
        cadmpeg_ir::features::FeatureDefinition::Helix {
            radius: cadmpeg_ir::features::Length(5.0),
            pitch: cadmpeg_ir::features::Length(0.0),
            revolutions: 3.5,
            radial_growth: Some(cadmpeg_ir::features::Length(2.0)),
            cone_angle: None,
            segment_turns: Some(0.25),
            construction_style: None,
            ..
        }
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_partdesign_refine_and_fuzzy_post_processing() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::AdditiveBox" name="Automatic" id="1"/>
 <Object type="PartDesign::AdditiveBox" name="Explicit" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Automatic"><Properties Count="5">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Refine" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloat"><Float value="-0.5"/></Property>
 </Properties></Object>
 <Object name="Explicit"><Properties Count="5">
  <Property name="Length" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="6"/></Property>
  <Property name="Refine" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="FuzzyTolerance" type="App::PropertyFloat"><Float value="0.01"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("PartDesign post-processing");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Automatic"),
        cadmpeg_ir::features::FeatureDefinition::PostProcess {
            operation,
            refine: true,
            fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::Automatic,
        } if matches!(operation.as_ref(), cadmpeg_ir::features::FeatureDefinition::Primitive { .. })
    ));
    assert!(matches!(
        definition("Explicit"),
        cadmpeg_ir::features::FeatureDefinition::PostProcess {
            operation,
            refine: false,
            fuzzy_tolerance: cadmpeg_ir::features::FuzzyTolerance::Explicit(0.01),
        } if matches!(operation.as_ref(), cadmpeg_ir::features::FeatureDefinition::Primitive { .. })
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_part_construction_geometry_features() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="8">
 <Object type="Part::Vertex" name="Vertex" id="1"/>
 <Object type="Part::Line" name="Line" id="2"/>
 <Object type="Part::Circle" name="Circle" id="3"/>
 <Object type="Part::Ellipse" name="Ellipse" id="4"/>
 <Object type="Part::Polygon" name="Polyline" id="5"/>
 <Object type="Part::RegularPolygon" name="Regular" id="6"/>
 <Object type="Part::Plane" name="Plane" id="7"/>
 <Object type="Part::Face" name="Face" id="8"/>
</Objects>
<ObjectData Count="8">
 <Object name="Vertex"><Properties Count="3"><Property name="X" type="App::PropertyDistance"><Float value="1"/></Property><Property name="Y" type="App::PropertyDistance"><Float value="2"/></Property><Property name="Z" type="App::PropertyDistance"><Float value="3"/></Property></Properties></Object>
 <Object name="Line"><Properties Count="6"><Property name="X1" type="App::PropertyDistance"><Float value="0"/></Property><Property name="Y1" type="App::PropertyDistance"><Float value="1"/></Property><Property name="Z1" type="App::PropertyDistance"><Float value="2"/></Property><Property name="X2" type="App::PropertyDistance"><Float value="3"/></Property><Property name="Y2" type="App::PropertyDistance"><Float value="4"/></Property><Property name="Z2" type="App::PropertyDistance"><Float value="5"/></Property></Properties></Object>
 <Object name="Circle"><Properties Count="3"><Property name="Radius" type="App::PropertyLength"><Float value="4"/></Property><Property name="Angle1" type="App::PropertyAngle"><Float value="30"/></Property><Property name="Angle2" type="App::PropertyAngle"><Float value="300"/></Property></Properties></Object>
 <Object name="Ellipse"><Properties Count="4"><Property name="MajorRadius" type="App::PropertyLength"><Float value="6"/></Property><Property name="MinorRadius" type="App::PropertyLength"><Float value="2"/></Property><Property name="Angle1" type="App::PropertyAngle"><Float value="15"/></Property><Property name="Angle2" type="App::PropertyAngle"><Float value="270"/></Property></Properties></Object>
 <Object name="Polyline"><Properties Count="2"><Property name="Nodes" type="App::PropertyVectorList"><VectorList count="3"><Vector x="0" y="0" z="0"/><Vector x="2" y="0" z="0"/><Vector x="1" y="1" z="0"/></VectorList></Property><Property name="Close" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="Regular"><Properties Count="2"><Property name="Polygon" type="App::PropertyInteger"><Integer value="7"/></Property><Property name="Circumradius" type="App::PropertyLength"><Float value="8"/></Property></Properties></Object>
 <Object name="Plane"><Properties Count="2"><Property name="Length" type="App::PropertyLength"><Float value="9"/></Property><Property name="Width" type="App::PropertyLength"><Float value="10"/></Property></Properties></Object>
 <Object name="Face"><Properties Count="2"><Property name="Sources" type="App::PropertyLinkList"><LinkList count="2"><Link value="Line"/><Link value="Circle"/></LinkList></Property><Property name="FaceMakerClass" type="App::PropertyString"><String value="Part::FaceMakerUnified"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part construction geometry");
    let feature = |name: &str| {
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    use cadmpeg_ir::features::FeatureDefinition;
    assert!(
        matches!(feature("Vertex").definition, FeatureDefinition::PointGeometry { position } if position == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
    );
    assert!(
        matches!(feature("Line").definition, FeatureDefinition::LineSegment { start, end } if start == cadmpeg_ir::math::Point3::new(0.0, 1.0, 2.0) && end == cadmpeg_ir::math::Point3::new(3.0, 4.0, 5.0))
    );
    assert!(matches!(
        feature("Circle").definition,
        FeatureDefinition::CircularArc {
            radius: cadmpeg_ir::features::Length(4.0),
            ..
        }
    ));
    assert!(matches!(
        feature("Ellipse").definition,
        FeatureDefinition::EllipticArc {
            major_radius: cadmpeg_ir::features::Length(6.0),
            minor_radius: cadmpeg_ir::features::Length(2.0),
            ..
        }
    ));
    assert!(
        matches!(&feature("Polyline").definition, FeatureDefinition::Polyline { points, closed: true } if points.len() == 3)
    );
    assert!(matches!(
        feature("Regular").definition,
        FeatureDefinition::RegularPolygonCurve {
            sides: 7,
            circumradius: cadmpeg_ir::features::Length(8.0)
        }
    ));
    assert!(matches!(
        feature("Plane").definition,
        FeatureDefinition::PlanarPatch {
            length: cadmpeg_ir::features::Length(9.0),
            width: cadmpeg_ir::features::Length(10.0)
        }
    ));
    assert!(
        matches!(&feature("Face").definition, FeatureDefinition::FaceFromShapes { sources: cadmpeg_ir::features::BodySelection::Native(source), face_maker_class } if source.ends_with(":Sources") && face_maker_class == "Part::FaceMakerUnified")
    );
    assert_eq!(feature("Face").dependencies.len(), 2);
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_uniform_and_anisotropic_part_scale() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="Part::Scale" name="Uniform" id="2"/>
 <Object type="Part::Scale" name="Anisotropic" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Source"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="1"/></Property>
 </Properties></Object>
 <Object name="Uniform"><Properties Count="3">
  <Property name="Base" type="App::PropertyLink"><Link value="Source"/></Property>
  <Property name="Uniform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="UniformScale" type="App::PropertyFloat"><Float value="-2"/></Property>
 </Properties></Object>
 <Object name="Anisotropic"><Properties Count="5">
  <Property name="Base" type="App::PropertyLink"><Link value="Source"/></Property>
  <Property name="Uniform" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="XScale" type="App::PropertyFloat"><Float value="2"/></Property>
  <Property name="YScale" type="App::PropertyFloat"><Float value="3"/></Property>
  <Property name="ZScale" type="App::PropertyFloat"><Float value="4"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part scale");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    assert!(matches!(
        definition("Uniform"),
        cadmpeg_ir::features::FeatureDefinition::Scale {
            center: Some(cadmpeg_ir::features::ScaleCenter::ModelOrigin),
            factors: cadmpeg_ir::features::ScaleFactors {
                uniform: Some(-2.0),
                x: None,
                y: None,
                z: None
            },
            ..
        }
    ));
    assert!(matches!(
        definition("Anisotropic"),
        cadmpeg_ir::features::FeatureDefinition::Scale {
            factors: cadmpeg_ir::features::ScaleFactors {
                uniform: None,
                x: Some(2.0),
                y: Some(3.0),
                z: Some(4.0)
            },
            ..
        }
    ));
}

#[test]
fn transfers_part_compound_refine_and_reverse_operations() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="Part::Box" name="A" id="1"/>
 <Object type="Part::Box" name="B" id="2"/>
 <Object type="Part::Compound" name="Compound" id="3"/>
 <Object type="Part::Compound2" name="Compound2" id="4"/>
 <Object type="Part::Refine" name="Refine" id="5"/>
 <Object type="Part::Reverse" name="Reverse" id="6"/>
</Objects>
<ObjectData Count="6">
 <Object name="A"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="B"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object>
 <Object name="Compound"><Properties Count="1"><Property name="Links" type="App::PropertyLinkList"><LinkList count="2"><Link value="A"/><Link value="B"/></LinkList></Property></Properties></Object>
 <Object name="Compound2"><Properties Count="1"><Property name="Links" type="App::PropertyLinkList"><LinkList count="2"><Link value="B"/><Link value="A"/></LinkList></Property></Properties></Object>
 <Object name="Refine"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Compound"/></Property></Properties></Object>
 <Object name="Reverse"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Refine"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("derived Part shapes");
    let feature = |name: &str| {
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    assert!(matches!(
        &feature("Compound").definition,
        cadmpeg_ir::features::FeatureDefinition::Compound {
            members: cadmpeg_ir::features::BodySelection::Native(reference)
        } if reference.ends_with(":Links")
    ));
    assert!(matches!(
        feature("Refine").definition,
        cadmpeg_ir::features::FeatureDefinition::RefineShape { .. }
    ));
    assert!(matches!(
        feature("Reverse").definition,
        cadmpeg_ir::features::FeatureDefinition::ReverseShape { .. }
    ));
    assert_eq!(feature("Compound").dependencies.len(), 2);
    assert_eq!(feature("Compound2").dependencies.len(), 2);
    assert_eq!(feature("Refine").dependencies.len(), 1);
    assert_eq!(feature("Reverse").dependencies.len(), 1);
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_part_ruled_surface_and_section_intersection() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="First" id="1"/>
 <Object type="Part::Box" name="Second" id="2"/>
 <Object type="Part::RuledSurface" name="Ruled" id="3"/>
 <Object type="Part::Section" name="Section" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="First"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Second"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object>
 <Object name="Ruled"><Properties Count="3">
  <Property name="Curve1" type="App::PropertyLinkSub"><Link object="First" sub="Edge1"/></Property>
  <Property name="Curve2" type="App::PropertyLinkSub"><Link object="Second" sub="Wire1"/></Property>
  <Property name="Orientation" type="App::PropertyEnumeration"><Integer value="2"/></Property>
 </Properties></Object>
 <Object name="Section"><Properties Count="3">
  <Property name="Base" type="App::PropertyLink"><Link value="First"/></Property>
  <Property name="Tool" type="App::PropertyLink"><Link value="Second"/></Property>
  <Property name="Approximation" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part surface constructions");
    let feature = |name: &str| {
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    };
    assert!(matches!(
        &feature("Ruled").definition,
        cadmpeg_ir::features::FeatureDefinition::RuledBetweenCurves {
            first: cadmpeg_ir::features::PathRef::Native(first),
            second: cadmpeg_ir::features::PathRef::Native(second),
            orientation: cadmpeg_ir::features::RuledCurveOrientation::Reversed,
        } if first.ends_with(":Curve1") && second.ends_with(":Curve2")
    ));
    assert!(matches!(
        feature("Section").definition,
        cadmpeg_ir::features::FeatureDefinition::SectionShape {
            approximate: true,
            ..
        }
    ));
    assert_eq!(feature("Ruled").dependencies.len(), 2);
    assert_eq!(feature("Section").dependencies.len(), 2);
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_standalone_part_mirror_plane_semantics() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="Part::Box" name="PlaneCarrier" id="2"/>
 <Object type="Part::Mirroring" name="Mirror" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Source"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="3"/></Property></Properties></Object>
 <Object name="PlaneCarrier"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="4"/></Property><Property name="Width" type="App::PropertyLength"><Float value="5"/></Property><Property name="Height" type="App::PropertyLength"><Float value="6"/></Property></Properties></Object>
 <Object name="Mirror"><Properties Count="4">
  <Property name="Source" type="App::PropertyLink"><Link value="Source"/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="1" y="2" z="3"/></Property>
  <Property name="Normal" type="App::PropertyVector"><Vector x="0" y="0" z="4"/></Property>
  <Property name="MirrorPlane" type="App::PropertyLinkSub"><Link object="PlaneCarrier" sub="Face1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("standalone Part mirror");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Mirror"))
        .expect("mirror feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::MirrorShape {
            source: cadmpeg_ir::features::BodySelection::Native(source),
            plane_origin: cadmpeg_ir::math::Point3 { x: 1.0, y: 2.0, z: 3.0 },
            plane_normal: cadmpeg_ir::math::Vector3 { x: 0.0, y: 0.0, z: 1.0 },
            plane_reference: Some(cadmpeg_ir::features::FaceSelection::Native(reference)),
        } if source.ends_with(":Source") && reference.ends_with(":MirrorPlane")
    ));
    assert_eq!(feature.dependencies.len(), 2);
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_part_projection_on_surface_construction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="First" id="1"/>
 <Object type="Part::Box" name="Second" id="2"/>
 <Object type="Part::Box" name="Support" id="3"/>
 <Object type="Part::ProjectOnSurface" name="Projection" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="First"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Second"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="2"/></Property><Property name="Width" type="App::PropertyLength"><Float value="2"/></Property><Property name="Height" type="App::PropertyLength"><Float value="2"/></Property></Properties></Object>
 <Object name="Support"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="3"/></Property><Property name="Width" type="App::PropertyLength"><Float value="3"/></Property><Property name="Height" type="App::PropertyLength"><Float value="3"/></Property></Properties></Object>
 <Object name="Projection"><Properties Count="6">
  <Property name="Projection" type="App::PropertyLinkSubList"><LinkList count="2"><Link object="First" sub="Wire1"/><Link object="Second" sub="Face2"/></LinkList></Property>
  <Property name="SupportFace" type="App::PropertyLinkSub"><Link object="Support" sub="Face1"/></Property>
  <Property name="Direction" type="App::PropertyVector"><Vector x="0" y="0" z="5"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Offset" type="App::PropertyDistance"><Float value="-1.5"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("projection on surface");
    let feature = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Projection"))
        .expect("projection feature");
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::ProjectOnSurface {
            sources: cadmpeg_ir::features::PathRef::Native(sources),
            support_face: cadmpeg_ir::features::FaceSelection::Native(support),
            direction: cadmpeg_ir::math::Vector3 { x: 0.0, y: 0.0, z: 1.0 },
            mode: cadmpeg_ir::features::SurfaceProjectionMode::Faces,
            height: cadmpeg_ir::features::Length(8.0),
            offset: cadmpeg_ir::features::Length(-1.5),
        } if sources.ends_with(":Projection")
            && support.ends_with(":SupportFace")
    ));
    assert_eq!(feature.dependencies.len(), 3);
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_ordered_part_boolean_operands_and_infers_dependencies() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Part::Box" name="A" id="1"/>
 <Object type="Part::Box" name="B" id="2"/>
 <Object type="Part::Box" name="C" id="3"/>
 <Object type="Part::Cut" name="Cut" id="4"/>
 <Object type="Part::MultiFuse" name="Fuse" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="A"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="B"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="C"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Cut"><Properties Count="2"><Property name="Base" type="App::PropertyLink"><Link value="A"/></Property><Property name="Tool" type="App::PropertyLink"><Link value="B"/></Property></Properties></Object>
 <Object name="Fuse"><Properties Count="1"><Property name="Shapes" type="App::PropertyLinkList"><LinkList count="2"><Link value="Cut"/><Link value="C"/></LinkList></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part booleans");
    let feature = |name: &str| {
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
    };
    assert!(matches!(
        feature("Cut").definition,
        cadmpeg_ir::features::FeatureDefinition::Combine {
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
    assert_eq!(
        feature("Cut")
            .dependencies
            .iter()
            .map(|id| id.0.as_str())
            .collect::<Vec<_>>(),
        ["fcstd:design:feature#A", "fcstd:design:feature#B"]
    );
    let cadmpeg_ir::features::FeatureDefinition::Combine { target, tools, op } =
        &feature("Fuse").definition
    else {
        panic!("multi-fuse");
    };
    assert_eq!(*op, cadmpeg_ir::features::BooleanOp::Join);
    assert!(matches!(
        target,
        cadmpeg_ir::features::BodySelection::Native(value) if value.ends_with(":link:0")
    ));
    assert!(matches!(
        tools,
        cadmpeg_ir::features::BodySelection::Native(value) if value.ends_with(":links:1..2")
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_partdesign_boolean_base_and_group_rules() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Part::Box" name="A" id="1"/><Object type="Part::Box" name="B" id="2"/><Object type="Part::Box" name="C" id="3"/>
 <Object type="PartDesign::Boolean" name="Fuse" id="4"/><Object type="PartDesign::Boolean" name="Cut" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="A"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="B"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="C"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Fuse"><Properties Count="2"><Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property><Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="A"/><Link value="B"/><Link value="C"/></LinkList></Property></Properties></Object>
 <Object name="Cut"><Properties Count="3"><Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property><Property name="BaseFeature" type="App::PropertyLink"><Link value="A"/></Property><Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="B"/><Link value="C"/></LinkList></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("PartDesign booleans");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("boolean")
            .definition
    };
    assert!(matches!(
        definition("Fuse"),
        cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(target),
            tools: cadmpeg_ir::features::BodySelection::Native(tools),
            op: cadmpeg_ir::features::BooleanOp::Join,
        } if target.ends_with(":Group:link:2")
            && tools.ends_with(":Group:links:0..2")
    ));
    assert!(matches!(
        definition("Cut"),
        cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(target),
            tools: cadmpeg_ir::features::BodySelection::Native(tools),
            op: cadmpeg_ir::features::BooleanOp::Cut,
        } if target.ends_with(":BaseFeature") && tools.ends_with(":Group")
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_ordered_loft_sections_and_subtractive_pipe_path() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
 <Object type="Sketcher::SketchObject" name="Section1" id="1"/>
 <Object type="Sketcher::SketchObject" name="Section2" id="2"/>
 <Object type="Sketcher::SketchObject" name="Path" id="3"/>
 <Object type="PartDesign::AdditiveLoft" name="Loft" id="4"/>
 <Object type="PartDesign::SubtractivePipe" name="Pipe" id="5"/>
 <Object type="Part::Loft" name="SurfaceLoft" id="6"/>
 <Object type="Part::Sweep" name="SurfaceSweep" id="7"/>
</Objects>
<ObjectData Count="7">
 <Object name="Section1"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Section2"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Path"><Properties Count="1"><Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property></Properties></Object>
 <Object name="Loft"><Properties Count="4">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Section1"/><Link value="Section2"/></LinkList></Property>
  <Property name="Closed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Ruled" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="Pipe"><Properties Count="11">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section1"/></Property>
  <Property name="Sections" type="App::PropertyLinkSubList"><LinkList count="2"><Link object="Section1"/><Link object="Section2"/></LinkList></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge1"/></Property>
  <Property name="SpineTangent" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AuxiliarySpine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge2"/></Property>
  <Property name="AuxiliarySpineTangent" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AuxiliaryCurvilinear" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="SurfaceLoft"><Properties Count="4">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Section1"/><Link value="Section2"/></LinkList></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="MaxDegree" type="App::PropertyInteger"><Integer value="7"/></Property>
  <Property name="CheckCompatibility" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="SurfaceSweep"><Properties Count="6">
  <Property name="Sections" type="App::PropertyLinkList"><LinkList count="2"><Link value="Section1"/><Link value="Section2"/></LinkList></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge1"/></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Frenet" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Linearize" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("loft and pipe");
    let feature = |name: &str| {
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
    };
    assert!(matches!(
        &feature("Loft").definition,
        cadmpeg_ir::features::FeatureDefinition::Loft {
            profiles,
            closed: true,
            solid: true,
            ruled: true,
            allow_multi_profile_faces: Some(false),
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        } if matches!(profiles.as_slice(), [
            cadmpeg_ir::features::ProfileRef::Sketch(first),
            cadmpeg_ir::features::ProfileRef::Sketch(second),
        ] if first.0.ends_with("#Section1") && second.0.ends_with("#Section2"))
    ));
    assert!(matches!(
        &feature("SurfaceLoft").definition,
        cadmpeg_ir::features::FeatureDefinition::Loft {
            solid: false,
            ruled: false,
            max_degree: Some(7),
            check_compatibility: Some(false),
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            ..
        }
    ));
    assert!(matches!(
        &feature("Pipe").definition,
        cadmpeg_ir::features::FeatureDefinition::Sweep {
            profile: Some(cadmpeg_ir::features::ProfileRef::Sketch(_)),
            sections,
            path: Some(cadmpeg_ir::features::PathRef::Native(path)),
            mode: cadmpeg_ir::features::SweepMode::Solid {
                op: cadmpeg_ir::features::BooleanOp::Cut,
            },
            orientation: Some(cadmpeg_ir::features::SweepOrientation::Auxiliary {
                tangent: true,
                curvilinear: false,
                ..
            }),
            transition: Some(cadmpeg_ir::features::SweepTransition::RoundCorner),
            transformation: Some(cadmpeg_ir::features::SweepTransformation::MultiSection),
            path_tangent: true,
            allow_multi_profile_faces: Some(true),
            ..
        } if path.ends_with(":Spine") && sections.len() == 1
    ));
    assert!(matches!(
        &feature("SurfaceSweep").definition,
        cadmpeg_ir::features::FeatureDefinition::Sweep {
            sections,
            mode: cadmpeg_ir::features::SweepMode::Surface,
            orientation: Some(cadmpeg_ir::features::SweepOrientation::CorrectedFrenet),
            transition: Some(cadmpeg_ir::features::SweepTransition::RoundCorner),
            transformation: Some(cadmpeg_ir::features::SweepTransformation::Constant),
            linearize: true,
            ..
        } if sections.len() == 1
    ));
    assert_eq!(feature("Loft").dependencies.len(), 2);
    assert_eq!(feature("Pipe").dependencies.len(), 3);
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_remaining_pipe_orientation_and_transformation_modes() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="8">
 <Object type="Sketcher::SketchObject" name="Section" id="1"/>
 <Object type="Sketcher::SketchObject" name="Path" id="2"/>
 <Object type="PartDesign::AdditivePipe" name="Fixed" id="3"/>
 <Object type="PartDesign::AdditivePipe" name="Frenet" id="4"/>
 <Object type="PartDesign::AdditivePipe" name="Binormal" id="5"/>
 <Object type="PartDesign::AdditivePipe" name="Linear" id="6"/>
 <Object type="PartDesign::AdditivePipe" name="SShape" id="7"/>
 <Object type="PartDesign::AdditivePipe" name="Interpolation" id="8"/>
</Objects>
<ObjectData Count="8">
 <Object name="Section"><Properties Count="0"/></Object>
 <Object name="Path"><Properties Count="0"/></Object>
 <Object name="Fixed"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge1"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
 <Object name="Frenet"><Properties Count="5">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge1"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
 <Object name="Binormal"><Properties Count="6">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge1"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="4"/></Property>
  <Property name="Binormal" type="App::PropertyVector"><Vector x="0" y="0" z="4"/></Property>
  <Property name="Transition" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="0"/></Property>
 </Properties></Object>
 <Object name="Linear"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge1"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="2"/></Property>
 </Properties></Object>
 <Object name="SShape"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge1"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="Interpolation"><Properties Count="3">
  <Property name="Profile" type="App::PropertyLink"><Link value="Section"/></Property>
  <Property name="Spine" type="App::PropertyLinkSub"><Link object="Path" sub="Edge1"/></Property>
  <Property name="Transformation" type="App::PropertyEnumeration"><Integer value="4"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("pipe modes");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, SweepOrientation, SweepTransformation, SweepTransition,
    };
    assert!(matches!(
        definition("Fixed"),
        FeatureDefinition::Sweep {
            orientation: Some(SweepOrientation::Fixed),
            transition: Some(SweepTransition::Transformed),
            ..
        }
    ));
    assert!(matches!(
        definition("Frenet"),
        FeatureDefinition::Sweep {
            orientation: Some(SweepOrientation::Frenet),
            transition: Some(SweepTransition::RightCorner),
            ..
        }
    ));
    assert!(matches!(
        definition("Binormal"),
        FeatureDefinition::Sweep {
            orientation: Some(SweepOrientation::Binormal { direction }),
            transition: Some(SweepTransition::RoundCorner),
            ..
        } if direction.z == 1.0
    ));
    for (name, expected) in [
        ("Linear", SweepTransformation::Linear),
        ("SShape", SweepTransformation::SShape),
        ("Interpolation", SweepTransformation::Interpolation),
    ] {
        assert!(matches!(
            definition(name),
            FeatureDefinition::Sweep {
                transformation: Some(actual),
                ..
            } if *actual == expected
        ));
    }
}

#[test]
fn transfers_uniform_irregular_and_two_axis_patterns() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="PartDesign::Feature" name="Seed" id="1"/>
 <Object type="PartDesign::LinearPattern" name="Uniform" id="2"/>
 <Object type="PartDesign::LinearPattern" name="Custom" id="3"/>
 <Object type="PartDesign::LinearPattern" name="TwoAxis" id="4"/>
 <Object type="PartDesign::PolarPattern" name="PolarCustom" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Seed"><Properties Count="0"/></Object>
 <Object name="Uniform"><Properties Count="7">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyVector"><Vector x="0" y="-1" z="0"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>
  <Property name="Occurrences2" type="App::PropertyInteger"><Integer value="1"/></Property>
 </Properties></Object>
 <Object name="Custom"><Properties Count="6">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyVector"><Vector x="1" y="0" z="0"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Offset" type="App::PropertyLength"><Float value="5"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
  <Property name="Spacings" type="App::PropertyFloatList"><FloatList count="2"><Float value="2"/><Float value="7"/></FloatList></Property>
 </Properties></Object>
 <Object name="TwoAxis"><Properties Count="11">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyVector"><Vector x="1" y="0" z="0"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
  <Property name="Direction2" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Reversed2" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Mode2" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Offset2" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Occurrences2" type="App::PropertyInteger"><Integer value="3"/></Property>
  <Property name="SpacingPattern2" type="App::PropertyFloatList"><FloatList count="2"><Float value="1"/><Float value="4"/></FloatList></Property>
 </Properties></Object>
 <Object name="PolarCustom"><Properties Count="7">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="0" z="1"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Offset" type="App::PropertyAngle"><Float value="30"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>
  <Property name="Spacings" type="App::PropertyFloatList"><FloatList count="3"><Float value="-1"/><Float value="-1"/><Float value="-1"/></FloatList></Property>
  <Property name="SpacingPattern" type="App::PropertyFloatList"><FloatList count="2"><Float value="10"/><Float value="20"/></FloatList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("linear patterns");
    let feature = |name: &str| {
        result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
    };
    assert!(matches!(
        &feature("Seed").definition,
        cadmpeg_ir::features::FeatureDefinition::StoredGeometry
    ));
    assert!(matches!(
        &feature("Uniform").definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            seeds,
            pattern: cadmpeg_ir::features::PatternKind::Linear {
                direction: Some(direction),
                spacing: cadmpeg_ir::features::Length(4.0),
                count: 4,
                ..
            },
        } if seeds.len() == 1 && direction.y == 1.0
    ));
    assert!(matches!(
        &feature("Custom").definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::LinearOffsets { direction: Some(direction), offsets },
            ..
        } if direction.x == 1.0 && offsets.iter().map(|offset| offset.0).collect::<Vec<_>>() == [0.0, 2.0, 9.0]
    ));
    let cadmpeg_ir::features::FeatureDefinition::Pattern {
        pattern: cadmpeg_ir::features::PatternKind::Composite { stages },
        ..
    } = &feature("TwoAxis").definition
    else {
        panic!("two-axis pattern")
    };
    assert_eq!(stages.len(), 2);
    assert!(matches!(
        *stages[0].pattern,
        cadmpeg_ir::features::PatternKind::Linear { count: 3, .. }
    ));
    assert!(matches!(
        &*stages[1].pattern,
        cadmpeg_ir::features::PatternKind::LinearOffsets { direction: Some(direction), offsets }
            if direction.y == -1.0 && offsets.iter().map(|offset| offset.0).collect::<Vec<_>>() == [0.0, 1.0, 5.0]
    ));
    assert_eq!(
        stages[1].combination,
        cadmpeg_ir::features::PatternStageCombination::CartesianProduct
    );
    assert!(matches!(
        &feature("PolarCustom").definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::CircularAngles { angles, .. },
            ..
        } if angles.iter().zip([0.0, 10.0, 30.0, 40.0]).all(|(angle, expected)|
            (angle.0.to_degrees() - expected).abs() < 1e-12)
    ));
    assert_eq!(feature("Uniform").dependencies.len(), 1);
    assert!(result.report.losses.is_empty());
    assert_valid_document(&result.ir);
    let census = result
        .ir
        .native
        .namespace("fcstd")
        .expect("native namespace")
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .expect("design census");
    assert_eq!(census.len(), 5);
    assert!(census.iter().any(|record| {
        record.object == "fcstd:native:object#Seed"
            && record.semantic_kind == "stored_geometry"
            && record.neutral
            && !record.post_processed
    }));
    assert!(census.iter().any(|record| {
        record.object == "fcstd:native:object#Custom"
            && record.semantic_kind == "pattern"
            && record.neutral
    }));
    let baseline_findings = cadmpeg_ir::validate(&result.ir, Vec::new()).findings;
    assert!(
        baseline_findings
            .iter()
            .all(|finding| finding.check != cadmpeg_ir::Check::Identity),
        "{baseline_findings:?}"
    );
    let mut corrupted = result.ir.clone();
    let mut stale_census = census;
    stale_census[0].neutral = !stale_census[0].neutral;
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("design_census", &stale_census)
        .expect("replace design census");
    let corrupted_findings = crate::validate_native(&corrupted);
    assert!(
        corrupted_findings.iter().any(|finding| finding
            .message
            .contains("design census does not match projected feature semantics")),
        "{corrupted_findings:?}"
    );
}

#[test]
fn distinguishes_stored_base_and_application_owned_features() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Feature" name="Source" id="1"/>
 <Object type="PartDesign::FeatureBase" name="BaseFeature" id="2"/>
 <Object type="Part::FeaturePython" name="PartExtension" id="3"/>
 <Object type="PartDesign::FeaturePython" name="DesignExtension" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Source"><Properties Count="0"/></Object>
 <Object name="BaseFeature"><Properties Count="1"><Property name="BaseFeature" type="App::PropertyLink"><Link value="Source"/></Property></Properties></Object>
 <Object name="PartExtension"><Properties Count="1"><Property name="ProxyState" type="App::PropertyString"><String value="part-owned"/></Property></Properties></Object>
 <Object name="DesignExtension"><Properties Count="1"><Property name="ProxyState" type="App::PropertyString"><String value="design-owned"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("stored and derived features");
    let source = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Source"))
        .expect("stored source");
    let base = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("BaseFeature"))
        .expect("base feature");
    assert!(matches!(
        source.definition,
        cadmpeg_ir::features::FeatureDefinition::StoredGeometry
    ));
    assert!(matches!(
        &base.definition,
        cadmpeg_ir::features::FeatureDefinition::DerivedGeometry { source }
            if source.0 == "fcstd:design:feature#Source"
    ));
    assert_eq!(base.dependencies, [source.id.clone()]);
    assert!(result.ir.model.features.iter().all(|feature| {
        !matches!(
            feature.name.as_deref(),
            Some("PartExtension" | "DesignExtension")
        )
    }));
    let namespace = result
        .ir
        .native
        .namespace("fcstd")
        .expect("native namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    assert!(objects
        .iter()
        .any(|object| object.type_name == "Part::FeaturePython"));
    assert!(objects
        .iter()
        .any(|object| object.type_name == "PartDesign::FeaturePython"));
    let census = namespace
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .expect("design census");
    assert_eq!(census.len(), 2);
    assert!(census.iter().all(|record| record.neutral));
    assert!(result.report.losses.is_empty());
    assert_valid_document(&result.ir);
    let mut corrupted = result.ir.clone();
    let derived = corrupted
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("BaseFeature"))
        .expect("derived feature");
    derived.definition = cadmpeg_ir::features::FeatureDefinition::DerivedGeometry {
        source: cadmpeg_ir::features::FeatureId("fcstd:design:feature#Missing".into()),
    };
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("source feature")));
}

#[test]
fn transfers_ordered_body_membership_and_active_tip() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="PartDesign::Body" name="Body" id="1"/>
 <Object type="PartDesign::Feature" name="First" id="2"/>
 <Object type="PartDesign::Feature" name="Second" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Body"><Properties Count="2">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="First"/><Link value="Second"/></LinkList></Property>
  <Property name="Tip" type="App::PropertyLink"><Link value="Second"/></Property>
 </Properties></Object>
 <Object name="First"><Properties Count="0"/></Object>
 <Object name="Second"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("body state");
    let body = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body"))
        .expect("body");
    let cadmpeg_ir::features::FeatureDefinition::TreeNode {
        children,
        active_child,
        ..
    } = &body.definition
    else {
        panic!("body tree node");
    };
    assert_eq!(
        children
            .iter()
            .map(|child| child.0.as_str())
            .collect::<Vec<_>>(),
        ["fcstd:design:feature#First", "fcstd:design:feature#Second"]
    );
    assert_eq!(active_child.as_ref(), children.get(1));
    for child in children {
        assert_eq!(
            result
                .ir
                .model
                .features
                .iter()
                .find(|feature| feature.id == *child)
                .and_then(|feature| feature.parent.as_ref()),
            Some(&body.id)
        );
    }
    assert!(result.report.losses.is_empty());
    assert_valid_document(&result.ir);

    let mut corrupted = result.ir.clone();
    let body = corrupted
        .model
        .features
        .iter_mut()
        .find(|feature| feature.name.as_deref() == Some("Body"))
        .expect("body");
    let cadmpeg_ir::features::FeatureDefinition::TreeNode { active_child, .. } =
        &mut body.definition
    else {
        panic!("body tree node");
    };
    *active_child = Some(cadmpeg_ir::features::FeatureId(
        "fcstd:design:feature#Outside".into(),
    ));
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("active tree child")));
}

#[test]
fn transfers_stored_and_external_part_feature_families() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="9">
 <Object type="Part::FeatureExt" name="Extended" id="1"/>
 <Object type="Part::FeatureGeometrySet" name="GeometrySet" id="2"/>
 <Object type="Part::Spline" name="Spline" id="3"/>
 <Object type="Part::Part2DObject" name="Planar" id="4"/>
 <Object type="Part::ImportStep" name="Step" id="5"/>
 <Object type="Part::ImportIges" name="Iges" id="6"/>
 <Object type="Part::ImportBrep" name="Brep" id="7"/>
 <Object type="Part::CurveNet" name="CurveNet" id="8"/>
 <Object type="Part::Part2DObjectPython" name="PlanarExtension" id="9"/>
</Objects>
<ObjectData Count="9">
 <Object name="Extended"><Properties Count="0"/></Object>
 <Object name="GeometrySet"><Properties Count="0"/></Object>
 <Object name="Spline"><Properties Count="0"/></Object>
 <Object name="Planar"><Properties Count="0"/></Object>
 <Object name="Step"><Properties Count="1"><Property name="FileName" type="App::PropertyString"><String value="models/source.step"/></Property></Properties></Object>
 <Object name="Iges"><Properties Count="1"><Property name="FileName" type="App::PropertyString"><String value="models/source.igs"/></Property></Properties></Object>
 <Object name="Brep"><Properties Count="1"><Property name="FileName" type="App::PropertyString"><String value="models/source.brep"/></Property></Properties></Object>
 <Object name="CurveNet"><Properties Count="1"><Property name="FileName" type="App::PropertyString"><String value="models/network.brep"/></Property></Properties></Object>
 <Object name="PlanarExtension"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("remaining Part feature families");
    for name in ["Extended", "GeometrySet", "Spline", "Planar"] {
        assert!(matches!(
            result
                .ir
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some(name))
                .expect("stored feature")
                .definition,
            cadmpeg_ir::features::FeatureDefinition::StoredGeometry
        ));
    }
    for (name, format) in [
        ("Step", cadmpeg_ir::features::GeometryImportFormat::Step),
        ("Iges", cadmpeg_ir::features::GeometryImportFormat::Iges),
        ("Brep", cadmpeg_ir::features::GeometryImportFormat::Brep),
        ("CurveNet", cadmpeg_ir::features::GeometryImportFormat::Brep),
    ] {
        assert!(matches!(
            &result
                .ir
                .model
                .features
                .iter()
                .find(|feature| feature.name.as_deref() == Some(name))
                .expect("import feature")
                .definition,
            cadmpeg_ir::features::FeatureDefinition::ImportedGeometry { path, format: actual }
                if path.starts_with("models/") && *actual == format
        ));
    }
    assert!(result
        .ir
        .model
        .features
        .iter()
        .all(|feature| feature.name.as_deref() != Some("PlanarExtension")));
    let census = result
        .ir
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .expect("design census");
    assert_eq!(census.len(), 8);
    assert!(census.iter().all(|record| record.neutral));
    assert!(result.report.losses.is_empty());
    assert_valid_document(&result.ir);
}

#[test]
fn resolves_datum_references_for_polar_and_mirror_patterns() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Part::Box" name="Seed" id="1"/>
 <Object type="PartDesign::Line" name="Axis" id="2"/>
 <Object type="PartDesign::Plane" name="Plane" id="3"/>
 <Object type="PartDesign::PolarPattern" name="Ring" id="4"/>
 <Object type="PartDesign::Mirrored" name="Mirror" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Seed"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Axis"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Plane"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Ring"><Properties Count="5">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Axis" type="App::PropertyLinkSub"><Link object="Axis" sub=""/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="360"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>
 </Properties></Object>
 <Object name="Mirror"><Properties Count="2">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="MirrorPlane" type="App::PropertyLinkSub"><Link object="Plane" sub=""/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("referenced patterns");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("pattern")
            .definition
    };
    assert!(matches!(
        definition("Ring"),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Circular {
                axis_origin,
                axis_dir,
                angle: cadmpeg_ir::features::Angle(angle),
                count: 4,
            },
            ..
        } if *axis_origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && *axis_dir == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
            && (*angle - std::f64::consts::TAU).abs() < 1e-12
    ));
    assert!(matches!(
        definition("Mirror"),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Mirror {
                plane_origin,
                plane_normal,
            },
            ..
        } if *plane_origin == cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
            && *plane_normal == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_progressive_scale_and_ordered_multi_transform_stages() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="Seed" id="1"/>
 <Object type="PartDesign::LinearPattern" name="Linear" id="2"/>
 <Object type="PartDesign::Scaled" name="Scaled" id="3"/>
 <Object type="PartDesign::MultiTransform" name="Multi" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Seed"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Linear"><Properties Count="5">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyVector"><Vector x="1" y="0" z="0"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="Scaled"><Properties Count="3">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Factor" type="App::PropertyFloat"><Float value="2.5"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="Multi"><Properties Count="2">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Transformations" type="App::PropertyLinkList"><LinkList count="2"><Link value="Linear"/><Link value="Scaled"/></LinkList></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("scaled multi-transform");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(matches!(
        definition("Scaled"),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Scale {
                center: cadmpeg_ir::features::PatternScaleCenter::FirstSeedCentroid,
                final_factor: 2.5,
                count: 3,
            },
            ..
        }
    ));
    let cadmpeg_ir::features::FeatureDefinition::Pattern {
        pattern: cadmpeg_ir::features::PatternKind::Composite { stages },
        ..
    } = definition("Multi")
    else {
        panic!("expected composite pattern");
    };
    assert_eq!(stages.len(), 2);
    assert_eq!(
        stages[0].combination,
        cadmpeg_ir::features::PatternStageCombination::Initialize
    );
    assert!(matches!(
        *stages[0].pattern,
        cadmpeg_ir::features::PatternKind::Linear { count: 3, .. }
    ));
    assert_eq!(
        stages[1].combination,
        cadmpeg_ir::features::PatternStageCombination::AlignedSlices
    );
    assert!(matches!(
        *stages[1].pattern,
        cadmpeg_ir::features::PatternKind::Scale { count: 3, .. }
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_complete_additive_and_outside_subtractive_helices() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
 <Object type="PartDesign::AdditiveHelix" name="Spring" id="2"/>
 <Object type="PartDesign::SubtractiveHelix" name="OutsideCut" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Profile"><Properties Count="0"/></Object>
 <Object name="Spring"><Properties Count="14">
  <Property name="Profile" type="App::PropertyLinkSub"><Link object="Profile" sub=""/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="1" y="2" z="3"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="0" z="1"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Pitch" type="App::PropertyLength"><Float value="4"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Turns" type="App::PropertyFloatConstraint"><Float value="2.5"/></Property>
  <Property name="Growth" type="App::PropertyDistance"><Float value="1"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="14.0362434679"/></Property>
  <Property name="LeftHanded" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Outside" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Tolerance" type="App::PropertyFloatConstraint"><Float value="0.25"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="OutsideCut"><Properties Count="13">
  <Property name="Profile" type="App::PropertyLinkSub"><Link object="Profile" sub=""/></Property>
  <Property name="Base" type="App::PropertyVector"><Vector x="0" y="0" z="0"/></Property>
  <Property name="Axis" type="App::PropertyVector"><Vector x="0" y="1" z="0"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="Pitch" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Turns" type="App::PropertyFloatConstraint"><Float value="3"/></Property>
  <Property name="Growth" type="App::PropertyDistance"><Float value="2"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="0"/></Property>
  <Property name="LeftHanded" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Outside" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Tolerance" type="App::PropertyFloatConstraint"><Float value="0.1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("helical sweeps");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(
        matches!(definition("Spring"), cadmpeg_ir::features::FeatureDefinition::HelicalSweep {
        construction,
        op: cadmpeg_ir::features::BooleanOp::Join,
    } if construction.law == cadmpeg_ir::features::HelicalSweepLaw::PitchTurnsAngle
        && construction.axis_origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
        && construction.left_handed && construction.reversed
        && construction.turns == 2.5 && construction.tolerance == 0.25
        && construction.allow_multi_profile_faces == Some(false))
    );
    assert!(
        matches!(definition("OutsideCut"), cadmpeg_ir::features::FeatureDefinition::HelicalSweep {
        construction,
        op: cadmpeg_ir::features::BooleanOp::Intersect,
    } if construction.law == cadmpeg_ir::features::HelicalSweepLaw::HeightTurnsGrowth
        && construction.pitch.0 == 0.0 && construction.radial_growth.0 == 2.0)
    );
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_remaining_partdesign_analytic_primitives() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="PartDesign::AdditiveEllipsoid" name="Ellipsoid" id="1"/>
 <Object type="PartDesign::SubtractivePrism" name="Prism" id="2"/>
 <Object type="PartDesign::AdditiveWedge" name="Wedge" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Ellipsoid"><Properties Count="6">
  <Property name="Radius1" type="App::PropertyLength"><Float value="3"/></Property><Property name="Radius2" type="App::PropertyLength"><Float value="5"/></Property><Property name="Radius3" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Angle1" type="App::PropertyAngle"><Float value="-45"/></Property><Property name="Angle2" type="App::PropertyAngle"><Float value="60"/></Property><Property name="Angle3" type="App::PropertyAngle"><Float value="270"/></Property>
 </Properties></Object>
 <Object name="Prism"><Properties Count="3"><Property name="Polygon" type="App::PropertyIntegerConstraint"><Integer value="7"/></Property><Property name="Circumradius" type="App::PropertyLength"><Float value="4"/></Property><Property name="Height" type="App::PropertyLength"><Float value="9"/></Property></Properties></Object>
 <Object name="Wedge"><Properties Count="10">
  <Property name="Xmin" type="App::PropertyDistance"><Float value="-2"/></Property><Property name="Ymin" type="App::PropertyDistance"><Float value="-1"/></Property><Property name="Zmin" type="App::PropertyDistance"><Float value="0"/></Property><Property name="X2min" type="App::PropertyDistance"><Float value="1"/></Property><Property name="Z2min" type="App::PropertyDistance"><Float value="2"/></Property>
  <Property name="Xmax" type="App::PropertyDistance"><Float value="8"/></Property><Property name="Ymax" type="App::PropertyDistance"><Float value="6"/></Property><Property name="Zmax" type="App::PropertyDistance"><Float value="10"/></Property><Property name="X2max" type="App::PropertyDistance"><Float value="7"/></Property><Property name="Z2max" type="App::PropertyDistance"><Float value="8"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("remaining primitives");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(
        matches!(definition("Ellipsoid"), cadmpeg_ir::features::FeatureDefinition::Primitive { solid: cadmpeg_ir::features::PrimitiveSolid::Ellipsoid { x_radius, y_radius, z_radius, .. }, op: cadmpeg_ir::features::BooleanOp::Join } if x_radius.0 == 5.0 && y_radius.0 == 5.0 && z_radius.0 == 3.0)
    );
    assert!(
        matches!(definition("Prism"), cadmpeg_ir::features::FeatureDefinition::Primitive { solid: cadmpeg_ir::features::PrimitiveSolid::Prism { sides: 7, circumradius, height }, op: cadmpeg_ir::features::BooleanOp::Cut } if circumradius.0 == 4.0 && height.0 == 9.0)
    );
    assert!(
        matches!(definition("Wedge"), cadmpeg_ir::features::FeatureDefinition::Primitive { solid: cadmpeg_ir::features::PrimitiveSolid::Wedge { xmin, ymax, .. }, op: cadmpeg_ir::features::BooleanOp::Join } if xmin.0 == -2.0 && ymax.0 == 6.0)
    );
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_shape_and_subshape_binder_construction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="Source" id="1"/>
 <Object type="PartDesign::CoordinateSystem" name="Context" id="2"/>
 <Object type="PartDesign::ShapeBinder" name="ShapeBind" id="3"/>
 <Object type="PartDesign::SubShapeBinder" name="SubBind" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Source"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Context"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="ShapeBind"><Properties Count="2"><Property name="Support" type="App::PropertyLinkSubListGlobal"><LinkList count="1"><Link object="Source" sub="Face1 Face2"/></LinkList></Property><Property name="TraceSupport" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="SubBind"><Properties Count="15">
  <Property name="Support" type="App::PropertyXLinkSubList"><LinkList count="2"><XLink object="Source" sub="Edge1"/><XLink document="library.FCStd" object="RemotePart" sub="Face3"/></LinkList></Property>
  <Property name="Context" type="App::PropertyXLink"><XLink object="Context"/></Property>
  <Property name="ClaimChildren" type="App::PropertyBool"><Bool value="true"/></Property><Property name="Relative" type="App::PropertyBool"><Bool value="false"/></Property><Property name="Fuse" type="App::PropertyBool"><Bool value="true"/></Property><Property name="MakeFace" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="BindMode" type="App::PropertyEnumeration"><Integer value="1"/></Property><Property name="PartialLoad" type="App::PropertyBool"><Bool value="true"/></Property><Property name="BindCopyOnChange" type="App::PropertyEnumeration"><Integer value="2"/></Property><Property name="Refine" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Offset" type="App::PropertyFloat"><Float value="-2.5"/></Property><Property name="OffsetJoinType" type="App::PropertyEnumeration"><Integer value="2"/></Property><Property name="OffsetFill" type="App::PropertyBool"><Bool value="true"/></Property><Property name="OffsetOpenResult" type="App::PropertyBool"><Bool value="true"/></Property><Property name="OffsetIntersection" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("binders");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("feature")
            .definition
    };
    assert!(
        matches!(definition("ShapeBind"), cadmpeg_ir::features::FeatureDefinition::Binder { sources, construction: cadmpeg_ir::features::BinderConstruction::Shape { trace_support: true } } if sources.len() == 1 && sources[0].subelements == ["Face1", "Face2"])
    );
    let cadmpeg_ir::features::FeatureDefinition::PostProcess {
        operation,
        refine: false,
        ..
    } = definition("SubBind")
    else {
        panic!("subshape binder post-processing");
    };
    let cadmpeg_ir::features::FeatureDefinition::Binder {
        sources,
        construction:
            cadmpeg_ir::features::BinderConstruction::SubShape {
                lifecycle,
                placement,
                copy_on_change,
                claim_children,
                fuse,
                make_face,
                partial_load,
                refine,
                offset: Some(offset),
                context: Some(context),
            },
    } = operation.as_ref()
    else {
        panic!("subshape binder");
    };
    assert_eq!(sources.len(), 2);
    assert!(
        matches!(sources[1].target, cadmpeg_ir::features::BinderTarget::External { ref document, ref object } if document == "library.FCStd" && object == "RemotePart")
    );
    assert_eq!(*lifecycle, cadmpeg_ir::features::BinderLifecycle::Frozen);
    assert_eq!(*placement, cadmpeg_ir::features::BinderPlacement::Global);
    assert_eq!(
        *copy_on_change,
        cadmpeg_ir::features::BinderCopyOnChange::Mutated
    );
    assert!(*claim_children && *fuse && !*make_face && *partial_load && !*refine);
    assert_eq!(offset.distance.0, -2.5);
    assert_eq!(
        offset.join,
        cadmpeg_ir::features::BinderOffsetJoin::Intersection
    );
    assert!(matches!(
        context,
        cadmpeg_ir::features::BinderTarget::Feature { .. }
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_complete_thickness_construction_controls() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Box" name="Base" id="1"/>
 <Object type="PartDesign::Thickness" name="Wall" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Base"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
 </Properties></Object>
 <Object name="Wall"><Properties Count="7">
  <Property name="Base" type="App::PropertyLinkSub"><Link object="Base" sub="Face2 Face4"/></Property>
  <Property name="Value" type="App::PropertyLength"><Float value="2.5"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Join" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Intersection" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="SelfIntersection" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("thickness");
    let wall = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Wall"))
        .expect("wall");
    assert!(matches!(
        &wall.definition,
        cadmpeg_ir::features::FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Native(selection),
            thickness: Some(cadmpeg_ir::features::Length(2.5)),
            outward: Some(false),
            mode: Some(cadmpeg_ir::features::ShellMode::BothSides),
            join: Some(cadmpeg_ir::features::ShellJoin::Tangent),
            resolve_intersections: Some(true),
            allow_self_intersections: Some(true),
            ..
        } if selection.ends_with(":Base")
    ));
    assert_eq!(wall.dependencies.len(), 1);
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_part_thickness_and_shape_offset_construction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="Base" id="1"/>
 <Object type="Part::Thickness" name="Thickness" id="2"/>
 <Object type="Part::Offset" name="Offset" id="3"/>
 <Object type="Part::Offset2D" name="Offset2D" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Base"><Properties Count="3">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Width" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Height" type="App::PropertyLength"><Float value="10"/></Property>
 </Properties></Object>
 <Object name="Thickness"><Properties Count="6">
  <Property name="Faces" type="App::PropertyLinkSub"><Link object="Base" sub="Face1 Face3"/></Property>
  <Property name="Value" type="App::PropertyLength"><Float value="-2"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Join" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Intersection" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="SelfIntersection" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Offset"><Properties Count="7">
  <Property name="Source" type="App::PropertyLink"><Link value="Base"/></Property>
  <Property name="Value" type="App::PropertyLength"><Float value="-1.5"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Join" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Intersection" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="SelfIntersection" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Fill" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Offset2D"><Properties Count="6">
  <Property name="Source" type="App::PropertyLink"><Link value="Base"/></Property>
  <Property name="Value" type="App::PropertyLength"><Float value="3"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Join" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Intersection" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Fill" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("Part offsets");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    use cadmpeg_ir::features::{FeatureDefinition, Length, ShellJoin, ShellMode};
    assert!(matches!(
        definition("Thickness"),
        FeatureDefinition::Shell {
            thickness: Some(Length(2.0)),
            outward: Some(false),
            mode: Some(ShellMode::Pipe),
            join: Some(ShellJoin::Intersection),
            resolve_intersections: Some(true),
            allow_self_intersections: Some(true),
            ..
        }
    ));
    assert!(matches!(
        definition("Offset"),
        FeatureDefinition::OffsetShape {
            distance: Length(-1.5),
            mode: ShellMode::BothSides,
            join: ShellJoin::Tangent,
            resolve_intersections: true,
            allow_self_intersections: true,
            fill: true,
            planar: false,
            ..
        }
    ));
    assert!(matches!(
        definition("Offset2D"),
        FeatureDefinition::OffsetShape {
            distance: Length(3.0),
            mode: ShellMode::Pipe,
            join: ShellJoin::Arc,
            fill: true,
            planar: true,
            ..
        }
    ));
}

#[test]
fn transfers_draft_with_resolved_neutral_plane_and_pull_direction() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Box" name="Base" id="1"/>
 <Object type="PartDesign::Plane" name="Neutral" id="2"/>
 <Object type="PartDesign::Line" name="Pull" id="3"/>
 <Object type="PartDesign::Draft" name="Draft" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Base"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="10"/></Property><Property name="Width" type="App::PropertyLength"><Float value="10"/></Property><Property name="Height" type="App::PropertyLength"><Float value="10"/></Property></Properties></Object>
 <Object name="Neutral"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="2" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Pull"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0.7071067811865476" Q1="0" Q2="0" Q3="0.7071067811865476"/></Property></Properties></Object>
 <Object name="Draft"><Properties Count="5">
  <Property name="Base" type="App::PropertyLinkSub"><Link object="Base" sub="Face1 Face3"/></Property>
  <Property name="NeutralPlane" type="App::PropertyLinkSub"><Link object="Neutral" sub=""/></Property>
  <Property name="PullDirection" type="App::PropertyLinkSub"><Link object="Pull" sub=""/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="5"/></Property>
  <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("draft");
    let draft = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Draft"))
        .expect("draft feature");
    assert!(matches!(
        &draft.definition,
        cadmpeg_ir::features::FeatureDefinition::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Native(faces),
            neutral_plane: cadmpeg_ir::features::FaceSelection::Native(plane),
            pull_direction,
            angle: Some(cadmpeg_ir::features::Angle(angle)),
            outward: Some(true),
        } if faces.ends_with(":Base")
            && plane.ends_with(":NeutralPlane")
            && pull_direction.is_some_and(|direction|
                (direction.x - 0.0).abs() < 1e-12
                    && (direction.y + 1.0).abs() < 1e-12
                    && direction.z.abs() < 1e-12)
            && (*angle + 5f64.to_radians()).abs() < 1e-12
    ));
    assert_eq!(draft.dependencies.len(), 3);
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_branch_complete_threaded_counterdrill_hole() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Sketcher::SketchObject" name="Locations" id="1"/>
 <Object type="PartDesign::Hole" name="Hole" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Locations"><Properties Count="2">
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
 <Object name="Hole"><Properties Count="26">
  <Property name="Profile" type="App::PropertyLink"><Link value="Locations"/></Property>
  <Property name="BaseProfileType" type="App::PropertyInteger"><Integer value="7"/></Property>
  <Property name="Diameter" type="App::PropertyLength"><Float value="6.8"/></Property>
  <Property name="HoleCutType" type="App::PropertyEnumeration"><Integer value="3"/></Property>
  <Property name="HoleCutDiameter" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="HoleCutDepth" type="App::PropertyLength"><Float value="2"/></Property>
  <Property name="HoleCutCountersinkAngle" type="App::PropertyAngle"><Float value="90"/></Property>
  <Property name="DepthType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="DrillPoint" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="DrillPointAngle" type="App::PropertyAngle"><Float value="118"/></Property>
  <Property name="DrillForDepth" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Tapered" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="TaperedAngle" type="App::PropertyAngle"><Float value="60"/></Property>
  <Property name="ThreadType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="ThreadSize" type="App::PropertyEnumeration"><Integer value="1"/><CustomEnumList count="2"><Enum value="M6"/><Enum value="M8"/></CustomEnumList></Property>
  <Property name="ThreadClass" type="App::PropertyEnumeration"><Integer value="0"/><CustomEnumList count="1"><Enum value="6H"/></CustomEnumList></Property>
  <Property name="Threaded" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="ModelThread" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="CosmeticThread" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="ThreadPitch" type="App::PropertyLength"><Float value="1.25"/></Property>
  <Property name="ThreadDiameter" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="ThreadDirection" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="ThreadDepthType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="ThreadDepth" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="UseCustomThreadClearance" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("hole");
    let hole = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Hole"))
        .expect("hole feature");
    let cadmpeg_ir::features::FeatureDefinition::Hole {
        profile,
        profile_filter,
        direction,
        kind,
        extent,
        bottom,
        taper_angle,
        specification,
        allow_multi_profile_faces,
        ..
    } = &hole.definition
    else {
        panic!("typed hole");
    };
    assert!(matches!(
        profile,
        Some(cadmpeg_ir::features::ProfileRef::Sketch(_))
    ));
    assert_eq!(
        *profile_filter,
        Some(cadmpeg_ir::features::HoleProfileFilter {
            points: true,
            circles: true,
            arcs: true,
        })
    );
    assert_eq!(
        *direction,
        Some(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0))
    );
    assert!(matches!(
        kind,
        cadmpeg_ir::features::HoleKind::Counterdrill {
            diameter: cadmpeg_ir::features::Length(12.0),
            depth: cadmpeg_ir::features::Length(2.0),
            angle: cadmpeg_ir::features::Angle(angle),
        } if (*angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12
    ));
    assert!(matches!(
        extent,
        Some(cadmpeg_ir::features::Extent::ThroughAll)
    ));
    assert!(matches!(
        bottom,
        Some(cadmpeg_ir::features::HoleBottom::Angled {
            depth_to_tip: true,
            ..
        })
    ));
    assert!(taper_angle.is_some());
    assert_eq!(*allow_multi_profile_faces, Some(true));
    let specification = specification.as_deref().expect("thread specification");
    assert_eq!(specification.standard, "ISO metric");
    assert_eq!(specification.designation.as_deref(), Some("M8"));
    assert_eq!(specification.class.as_deref(), Some("6H"));
    assert!(specification.threaded && specification.modeled && !specification.cosmetic);
    assert_eq!(specification.hand, cadmpeg_ir::features::ThreadHand::Left);
    assert!(matches!(
        specification.depth,
        cadmpeg_ir::features::HoleThreadDepth::Blind {
            depth: cadmpeg_ir::features::Length(12.0)
        }
    ));
    assert_eq!(hole.dependencies.len(), 1);
    assert!(result.report.losses.is_empty());
    let findings = cadmpeg_ir::validate(&result.ir, Vec::new()).findings;
    assert!(
        findings
            .iter()
            .all(|finding| finding.check != cadmpeg_ir::Check::GeometricConsistency),
        "{findings:#?}"
    );
}

#[test]
fn transfers_datum_frames_from_persisted_placements() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="PartDesign::Plane" name="Plane" id="1"/>
 <Object type="PartDesign::Line" name="Axis" id="2"/>
 <Object type="PartDesign::Point" name="Point" id="3"/>
 <Object type="PartDesign::CoordinateSystem" name="Frame" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Plane"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Axis"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Point"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="7" Py="8" Pz="9" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Frame"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="11" Pz="12" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("datums");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .expect("datum")
            .definition
    };
    assert!(matches!(
        definition("Plane"),
        cadmpeg_ir::features::FeatureDefinition::DatumPlane { origin, normal, u_axis }
            if *origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && *normal == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                && *u_axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
    ));
    assert!(matches!(
        definition("Axis"),
        cadmpeg_ir::features::FeatureDefinition::DatumAxis { origin, direction }
            if *origin == cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
                && *direction == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    ));
    assert!(matches!(
        definition("Point"),
        cadmpeg_ir::features::FeatureDefinition::DatumPoint { position }
            if *position == cadmpeg_ir::math::Point3::new(7.0, 8.0, 9.0)
    ));
    assert!(matches!(
        definition("Frame"),
        cadmpeg_ir::features::FeatureDefinition::DatumCoordinateSystem { origin, x_axis, y_axis, z_axis }
            if *origin == cadmpeg_ir::math::Point3::new(10.0, 11.0, 12.0)
                && *x_axis == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
                && *y_axis == cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
                && *z_axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn reports_attributable_native_design_blockers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::FeatureBase" name="Custom" id="1"/></Objects>
<ObjectData Count="1"><Object name="Custom"><Properties Count="2"><Property name="Refine" type="App::PropertyBool"><Bool value="true"/></Property><Property name="FuzzyTolerance" type="App::PropertyFloat"><Float value="0"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("native feature");
    assert_eq!(result.report.losses.len(), 1);
    assert_eq!(
        result.report.losses[0].severity,
        cadmpeg_ir::Severity::Blocking
    );
    assert_eq!(
        result.report.losses[0]
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("fcstd:native:object#Custom")
    );
}

#[test]
fn transfers_spreadsheet_cells_aliases_and_parameter_dependencies() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Spreadsheet::Sheet" name="Sheet" id="1"/>
 <Object type="PartDesign::Pad" name="Pad" id="2"/>
 <ObjectDeps Name="Pad"><Dep Name="Sheet"/></ObjectDeps>
</Objects>
<ObjectData Count="2">
 <Object name="Sheet"><Properties Count="3"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="2" xlink="1">
  <Cell address="A1" content="5" alias="width" displayUnit="mm" rowSpan="1" colSpan="2"/>
  <Cell address="A2" content="=width * 3" alias="height" style="bold"/>
 </Cells></Property>
 <Property name="columnWidths" type="Spreadsheet::PropertyColumnWidths"><ColumnInfo Count="2"><Column name="A" width="120"/><Column name="B" width="80"/></ColumnInfo></Property>
 <Property name="rowHeights" type="Spreadsheet::PropertyRowHeights"><RowInfo Count="1"><Row name="2" height="45"/></RowInfo></Property>
 </Properties></Object>
 <Object name="Pad"><Properties Count="2">
  <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Length" expression="Sheet.width * 2"/></ExpressionEngine></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("spreadsheet");
    let width = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == "width")
        .expect("width cell");
    assert_eq!(
        width.value,
        Some(cadmpeg_ir::features::ParameterValue::Real(5.0))
    );
    assert_eq!(
        width.properties.get("address").map(String::as_str),
        Some("A1")
    );
    let pad = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .expect("pad");
    let length = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner == pad.id && parameter.name == "Length")
        .expect("pad length");
    assert_eq!(length.dependencies, vec![width.id.clone()]);
    let sheet = result.ir.model.spreadsheets.first().expect("sheet state");
    assert_eq!(sheet.feature.0, "fcstd:design:feature#Sheet");
    assert_eq!(sheet.cells.len(), 2);
    assert_eq!(
        sheet.column_widths,
        [
            cadmpeg_ir::SpreadsheetDimension {
                name: "A".into(),
                pixels: 120,
            },
            cadmpeg_ir::SpreadsheetDimension {
                name: "B".into(),
                pixels: 80,
            },
        ]
    );
    assert_eq!(
        sheet.row_heights,
        [cadmpeg_ir::SpreadsheetDimension {
            name: "2".into(),
            pixels: 45,
        }]
    );
    assert_eq!(
        sheet.merged_ranges,
        [cadmpeg_ir::SpreadsheetRange {
            start: "A1".into(),
            end: "B1".into(),
        }]
    );
    assert_valid_document(&result.ir);
    let mut corrupted = result.ir.clone();
    corrupted.model.spreadsheets[0]
        .merged_ranges
        .push(cadmpeg_ir::SpreadsheetRange {
            start: "A1".into(),
            end: "A2".into(),
        });
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("merged ranges overlap")));
}

#[test]
fn recovers_product_prototypes_occurrences_and_placements() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="App::Part" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="Prototype" id="2"/>
 <Object type="App::Link" name="Occurrence" id="3"/>
 <Object type="Part::Feature" name="ElementA" id="4"/>
 <Object type="Part::Feature" name="ElementB" id="5"/>
 <Object type="App::Part" name="Outer" id="6"/>
</Objects>
<ObjectData Count="6">
 <Object name="Assembly"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Occurrence"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Prototype"><Properties Count="3"><Property name="Label" type="App::PropertyString"><String value="Drive gear"/></Property><Property name="Description" type="App::PropertyString"><String value="Hardened drive gear"/></Property><Property name="PartNumber" type="App::PropertyString"><String value="GEAR-42"/></Property></Properties></Object>
 <Object name="Occurrence"><Properties Count="14">
  <Property name="LinkedObject" type="App::PropertyXLinkSub"><XLink file="" name="Prototype" count="1"><Sub value="Face1"/></XLink></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="2"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="PlacementList"/></Property>
  <Property name="ScaleList" type="App::PropertyVectorList"><VectorList file="ScaleList"/></Property>
  <Property name="ScaleVector" type="App::PropertyVector"><PropertyVector valueX="2" valueY="3" valueZ="4"/></Property>
  <Property name="VisibilityList" type="App::PropertyBoolList"><BoolList count="2"><Bool value="true"/><Bool value="false"/></BoolList></Property>
  <Property name="ElementList" type="App::PropertyLinkList"><LinkList count="2"><Link value="ElementA"/><Link value="ElementB"/></LinkList></Property>
  <Property name="LinkClaimChild" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="LinkCopyOnChange" type="App::PropertyEnumeration"><Integer value="2"/><CustomEnumList count="4"><Enum value="Disabled"/><Enum value="Enabled"/><Enum value="Owned"/><Enum value="Tracking"/></CustomEnumList></Property>
  <Property name="LinkCopyOnChangeSource" type="App::PropertyLink"><Link value="Prototype"/></Property>
  <Property name="LinkCopyOnChangeGroup" type="App::PropertyLink"><Link value="Assembly"/></Property>
  <Property name="LinkCopyOnChangeTouched" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="ElementA"><Properties Count="0"/></Object>
 <Object name="ElementB"><Properties Count="0"/></Object>
 <Object name="Outer"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Assembly"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="100" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let mut placements = 2_u32.to_le_bytes().to_vec();
    for value in [
        1.0_f64, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 4.0, 5.0, 6.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        placements.extend_from_slice(&value.to_le_bytes());
    }
    let mut scales = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f64, 1.0, 1.0, 2.0, 2.0, 2.0] {
        scales.extend_from_slice(&value.to_le_bytes());
    }
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("PlacementList", &placements),
        ("ScaleList", &scales),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("product structure");
    let nodes = result
        .ir
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::ProductNodeRecord>("product_nodes")
        .expect("product nodes");
    assert_eq!(nodes.len(), 3);
    let assembly = nodes
        .iter()
        .find(|node| node.object.ends_with("Assembly"))
        .expect("assembly part");
    let occurrence = nodes
        .iter()
        .find(|node| node.kind == "occurrence")
        .expect("occurrence");
    assert_eq!(assembly.members, vec![occurrence.object.clone()]);
    assert_eq!(
        occurrence.prototype.as_deref(),
        Some("fcstd:native:object#Prototype")
    );
    assert_eq!(occurrence.local_transform.expect("placement")[0][3], 4.0);
    assert_eq!(occurrence.element_count, Some(2));
    assert_eq!(occurrence.link_transform, Some(true));
    assert_eq!(occurrence.element_transforms.len(), 2);
    assert_eq!(occurrence.element_transforms[1][0][3], 4.0);
    assert_eq!(occurrence.element_scales, vec![[1.0; 3], [2.0; 3]]);
    assert_eq!(result.ir.model.components.len(), 5);
    let component = result
        .ir
        .model
        .components
        .iter()
        .find(|component| {
            component
                .native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with("Assembly"))
        })
        .expect("neutral assembly component");
    assert!(component.parent.is_some());
    assert_eq!(component.local_transform[0][3], 10.0);
    assert_eq!(component.resolved_transform[0][3], 110.0);
    assert_eq!(component.occurrences.len(), 2);
    assert_eq!(result.ir.model.occurrences.len(), 2);
    assert_eq!(result.ir.model.occurrences[0].array_index, Some(0));
    assert_eq!(result.ir.model.occurrences[0].local_transform[0][3], 5.0);
    assert_eq!(result.ir.model.occurrences[1].local_transform[0][3], 8.0);
    assert_eq!(
        result.ir.model.occurrences[0].resolved_transform[0][3],
        115.0
    );
    assert_eq!(
        result.ir.model.occurrences[1].resolved_transform[0][3],
        118.0
    );
    assert_eq!(result.ir.model.occurrences[0].scale, [2.0, 3.0, 4.0]);
    assert_eq!(result.ir.model.occurrences[1].scale, [4.0, 6.0, 8.0]);
    assert_eq!(result.ir.model.occurrences[0].linked_subelements, ["Face1"]);
    assert_eq!(result.ir.model.occurrences[0].visible, Some(true));
    assert_eq!(result.ir.model.occurrences[1].visible, Some(false));
    assert!(result.ir.model.occurrences[0].element_component.is_some());
    assert_eq!(result.ir.model.occurrences[0].claim_child, Some(true));
    assert_eq!(
        result.ir.model.occurrences[0].copy_on_change,
        Some(cadmpeg_ir::CopyOnChangePolicy::Owned)
    );
    assert!(result.ir.model.occurrences[0]
        .copy_on_change_source
        .is_some());
    assert!(result.ir.model.occurrences[0]
        .copy_on_change_group
        .is_some());
    assert_eq!(
        result.ir.model.occurrences[0].copy_on_change_touched,
        Some(true)
    );
    assert!(matches!(
        &result.ir.model.occurrences[0].prototype,
        cadmpeg_ir::ComponentReference::Local { component }
            if component.0.contains("Prototype")
    ));
    let prototype = result
        .ir
        .model
        .components
        .iter()
        .find(|component| component.source_name.as_deref() == Some("Prototype"))
        .expect("prototype component identity");
    assert_eq!(prototype.label.as_deref(), Some("Drive gear"));
    assert_eq!(
        prototype.description.as_deref(),
        Some("Hardened drive gear")
    );
    assert_eq!(prototype.part_number.as_deref(), Some("GEAR-42"));
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);
    let mut corrupted = result.ir.clone();
    corrupted.model.occurrences[0].prototype = cadmpeg_ir::ComponentReference::Local {
        component: cadmpeg_ir::ComponentId("fcstd:model:component#missing".into()),
    };
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid occurrence reference")));
    let mut corrupted = result.ir.clone();
    corrupted.model.occurrences[0].resolved_transform[0][3] += 1.0;
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding
            .message
            .contains("invalid occurrence reference or transform")));
}

#[test]
fn recovers_assembly_joint_operands_frames_and_state() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Assembly::AssemblyObject" name="Assembly" id="1"/>
 <Object type="App::FeaturePython" name="Joint" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Assembly"><Properties Count="0"/></Object>
 <Object name="Joint"><Properties Count="14">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1"/><CustomEnumList count="2"><Enum value="Fixed"/><Enum value="Revolute"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="2"><Sub value="A.Face1"/><Sub value="A.Edge2"/></XLink></Property>
  <Property name="Reference2" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="1"><Sub value="B.Edge3"/></XLink></Property>
  <Property name="Placement1" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement2" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="15"/></Property>
  <Property name="AngleMin" type="App::PropertyAngle"><Float value="-30"/></Property>
  <Property name="AngleMax" type="App::PropertyAngle"><Float value="45"/></Property>
  <Property name="EnableAngleMin" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="EnableAngleMax" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Detach1" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Offset1" type="App::PropertyPlacement"><PropertyPlacement Px="0.5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Offset2" type="App::PropertyPlacement"><PropertyPlacement Px="1.5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("joint");
    let joints = result
        .ir
        .native
        .namespace("fcstd")
        .expect("native")
        .arena_as::<crate::native::JointRecord>("joints")
        .expect("joints");
    assert_eq!(joints.len(), 1);
    assert_eq!(joints[0].kind, "Revolute");
    assert_eq!(joints[0].references.len(), 2);
    assert_eq!(
        joints[0].references[0].object.as_deref(),
        Some("fcstd:native:object#Assembly")
    );
    assert_eq!(joints[0].references[0].subelements, ["A.Face1", "A.Edge2"]);
    assert_eq!(joints[0].placements[1][0][3], 2.0);
    assert_eq!(
        joints[0].parameters.get("Suppressed").map(String::as_str),
        Some("true")
    );
    assert_eq!(result.ir.model.assembly_joints.len(), 1);
    let joint = &result.ir.model.assembly_joints[0];
    assert_eq!(joint.kind, cadmpeg_ir::JointKind::Revolute);
    assert_eq!(joint.operands.len(), 2);
    assert!(joint
        .operands
        .iter()
        .all(|operand| operand.component.is_some()));
    assert_eq!(joint.frames[1][0][3], 2.0);
    assert_eq!(joint.offset_frames.len(), 2);
    assert_eq!(joint.offset_frames[0][0][3], 0.5);
    assert_eq!(joint.offset_frames[1][0][3], 1.5);
    assert!(joint.suppressed);
    assert_eq!(joint.detached, [true, false]);
    assert!((joint.angle.expect("angle") - 15_f64.to_radians()).abs() < 1e-12);
    let limits = joint.angular_limits.as_ref().expect("angular limits");
    assert!((limits.minimum.expect("minimum") - (-30_f64).to_radians()).abs() < 1e-12);
    assert!((limits.maximum.expect("maximum") - 45_f64.to_radians()).abs() < 1e-12);
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);
    let mut corrupted = result.ir.clone();
    let limits = corrupted.model.assembly_joints[0]
        .angular_limits
        .as_mut()
        .expect("limits");
    limits.minimum = Some(2.0);
    limits.maximum = Some(1.0);
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid assembly joint")));
    let mut corrupted = result.ir.clone();
    corrupted.model.assembly_joints[0].operands[0].component = None;
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid assembly joint operands")));
}

#[test]
fn composes_nested_link_prototype_placements_once_by_policy() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="App::Part" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="Prototype" id="2"/>
 <Object type="App::Link" name="Inner" id="3"/>
 <Object type="App::Link" name="Outer" id="4"/>
 <Object type="App::Link" name="Override" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Assembly"><Properties Count="2"><Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="Outer"/><Link value="Override"/></LinkList></Property><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Prototype"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="5" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Inner"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyLink"><Link value="Prototype"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="3" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="Outer"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyLink"><Link value="Inner"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property></Properties></Object>
 <Object name="Override"><Properties Count="3"><Property name="LinkedObject" type="App::PropertyLink"><Link value="Inner"/></Property><Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property><Property name="LinkTransform" type="App::PropertyBool"><Bool value="false"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("nested links");
    let occurrence = |name: &str| {
        result
            .ir
            .model
            .occurrences
            .iter()
            .find(|occurrence| {
                occurrence
                    .native_ref
                    .as_deref()
                    .is_some_and(|id| id.ends_with(name))
            })
            .expect("named occurrence")
    };
    assert_eq!(occurrence("Inner").prototype_transform[0][3], 5.0);
    assert_eq!(occurrence("Inner").resolved_transform[0][3], 8.0);
    assert_eq!(occurrence("Outer").prototype_transform[0][3], 8.0);
    assert_eq!(occurrence("Outer").resolved_transform[0][3], 20.0);
    assert_eq!(occurrence("Override").prototype_transform[0][3], 0.0);
    assert_eq!(occurrence("Override").resolved_transform[0][3], 14.0);
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);
    let inner_component = result
        .ir
        .model
        .components
        .iter()
        .find(|component| component.source_name.as_deref() == Some("Inner"))
        .expect("inner link component")
        .id
        .clone();
    let mut corrupted = result.ir.clone();
    let inner = corrupted
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| {
            occurrence
                .native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with("Inner"))
        })
        .expect("inner link occurrence");
    inner.prototype = cadmpeg_ir::ComponentReference::Local {
        component: inner_component,
    };
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("prototype cycle")));
}

#[test]
fn distinguishes_external_product_paths_document_ids_and_targets() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="App::Link" name="ByPath" id="1"/>
 <Object type="App::Link" name="ByDocument" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="ByPath"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="parts/widget.FCStd" name="Body"/></Property></Properties></Object>
 <Object name="ByDocument"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink document="document-7" name="Gear"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("external products");
    assert_eq!(result.ir.model.occurrences.len(), 2);
    let by_path = result
        .ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence
                .native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with("ByPath"))
        })
        .expect("path occurrence");
    let cadmpeg_ir::ComponentReference::External { document, object } = &by_path.prototype else {
        panic!("path prototype is external");
    };
    assert_eq!(document.path.as_deref(), Some("parts/widget.FCStd"));
    assert_eq!(document.document_id, None);
    assert_eq!(object.as_deref(), Some("Body"));
    assert_eq!(
        document.resolution,
        cadmpeg_ir::ExternalResolution::Unresolved
    );

    let by_document = result
        .ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence
                .native_ref
                .as_deref()
                .is_some_and(|id| id.ends_with("ByDocument"))
        })
        .expect("document occurrence");
    let cadmpeg_ir::ComponentReference::External { document, object } = &by_document.prototype
    else {
        panic!("document prototype is external");
    };
    assert_eq!(document.path, None);
    assert_eq!(document.document_id.as_deref(), Some("document-7"));
    assert_eq!(object.as_deref(), Some("Gear"));
    assert_eq!(
        document.resolution,
        cadmpeg_ir::ExternalResolution::Unresolved
    );
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);
    let mut corrupted = result.ir.clone();
    let cadmpeg_ir::ComponentReference::External { document, .. } =
        &mut corrupted.model.occurrences[0].prototype
    else {
        panic!("external prototype");
    };
    document.path = Some("also-a-path.FCStd".into());
    document.document_id = Some("also-an-id".into());
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid occurrence reference")));
}

#[test]
fn transfers_grounded_assembly_state_with_resolved_component() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Part::Feature" name="BasePlate" id="1"/>
 <Object type="App::FeaturePython" name="Ground" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="BasePlate"><Properties Count="0"/></Object>
 <Object name="Ground"><Properties Count="2">
  <Property name="ObjectToGround" type="App::PropertyLink"><Link value="BasePlate"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="7" Py="8" Pz="9" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("grounded assembly object");
    assert_eq!(result.ir.model.assembly_joints.len(), 1);
    let joint = &result.ir.model.assembly_joints[0];
    assert_eq!(joint.kind, cadmpeg_ir::JointKind::Grounded);
    assert_eq!(joint.operands.len(), 1);
    assert!(joint.operands[0].component.is_some());
    assert_eq!(joint.frames.len(), 1);
    assert_eq!(joint.frames[0][0][3], 7.0);
    assert_eq!(joint.frames[0][1][3], 8.0);
    assert_eq!(joint.frames[0][2][3], 9.0);
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);
}

#[test]
fn censuses_application_domains_and_keeps_python_payloads_inert() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Mesh::Feature" name="Mesh" id="1"/>
 <Object type="Points::Feature" name="Points" id="2"/>
 <Object type="Fem::FemAnalysis" name="Analysis" id="3"/>
 <Object type="Path::FeaturePython" name="Toolpath" id="4"/>
 <Object type="LocalType" name="Local" id="5"/>
 <ObjectDeps Name="Mesh"><Dep Name="Points"/></ObjectDeps>
</Objects>
<ObjectData Count="5">
 <Object name="Mesh"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Points"/></Property></Properties></Object>
 <Object name="Points"><Properties Count="0"/></Object>
 <Object name="Analysis"><Properties Count="1"><Property name="Report" type="App::PropertyFileIncluded"><FileIncluded file="analysis.dat"/></Property></Properties></Object>
 <Object name="Toolpath"><Properties Count="1"><Property name="Proxy" type="App::PropertyPythonObject"><PythonObject class="ToolController">serialized-but-inert</PythonObject></Property></Properties></Object>
 <Object name="Local"><Properties Count="0"/></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("analysis.dat", b"finite-element-results"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("application census");
    let namespace = result.ir.native.namespace("fcstd").expect("native");
    let records = namespace
        .arena_as::<crate::native::ApplicationRecord>("applications")
        .expect("applications");
    assert_eq!(records.len(), 5);
    let by_domain = records
        .iter()
        .map(|record| (record.domain.as_str(), record))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        by_domain["Mesh"].dependencies,
        ["fcstd:native:object#Points"]
    );
    assert_eq!(by_domain["Fem"].side_entries, ["analysis.dat"]);
    assert!(by_domain["Path"].inert_payload);
    assert!(!by_domain["Mesh"].inert_payload);
    assert_eq!(by_domain["Unqualified"].type_name, "LocalType");
    let report = &by_domain["Fem"].property_records[0];
    assert_eq!(report.object, by_domain["Fem"].object);
    assert!(report.byte_start < report.byte_end);
    assert_eq!(report.byte_len, report.data.len() as u64);
    assert_eq!(report.sha256, cadmpeg_ir::hash::sha256_hex(&report.data));
    assert_eq!(report.payloads.len(), 1);
    assert_eq!(report.payloads[0].name, "analysis.dat");
    assert_eq!(report.payloads[0].data, b"finite-element-results");
    assert_eq!(
        report.payloads[0].sha256,
        cadmpeg_ir::hash::sha256_hex(&report.payloads[0].data)
    );
    let python = &by_domain["Path"].property_records[0];
    assert!(python.inert);
    assert!(String::from_utf8_lossy(&python.data).contains("serialized-but-inert"));
    assert!(records.iter().all(|record| {
        record.byte_start < record.byte_end
            && record.byte_len == record.data.len() as u64
            && record.sha256 == cadmpeg_ir::hash::sha256_hex(&record.data)
    }));
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);

    let mut corrupted = result.ir.clone();
    let mut stale_records = records.clone();
    stale_records[0].property_records[0].sha256 = "0".repeat(64);
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("applications", &stale_records)
        .expect("replace application records");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding
            .message
            .contains("application preservation records do not match authoritative bytes")));
}

#[test]
fn transfers_application_mesh_and_transformed_point_cloud_payloads() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Mesh::Feature" name="Mesh" id="1"/>
 <Object type="Points::Feature" name="Cloud" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Mesh"><Properties Count="1"><Property name="Mesh" type="Mesh::PropertyMeshKernel"><Mesh file="MeshKernel.bms"/></Property></Properties></Object>
 <Object name="Cloud"><Properties Count="1"><Property name="Points" type="Points::PropertyPointKernel"><Points file="Cloud" mtrx="1 0 0 10 0 1 0 20 0 0 1 30 0 0 0 1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let mut mesh = Vec::new();
    mesh.extend_from_slice(&0xa0b0_c0d0_u32.to_le_bytes());
    mesh.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    mesh.extend_from_slice(&[0; 256]);
    mesh.extend_from_slice(&3_u32.to_le_bytes());
    mesh.extend_from_slice(&1_u32.to_le_bytes());
    for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_u32, 1, 2, u32::MAX, u32::MAX, u32::MAX] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f32, 1.0, 0.0, 1.0, 0.0, 0.0] {
        mesh.extend_from_slice(&value.to_le_bytes());
    }
    let mut points = 2_u32.to_le_bytes().to_vec();
    for value in [1.0_f32, 2.0, 3.0, -1.0, -2.0, -3.0] {
        points.extend_from_slice(&value.to_le_bytes());
    }
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("MeshKernel.bms", &mesh),
                ("Cloud", &points),
            ])),
            &DecodeOptions::default(),
        )
        .expect("application geometry");
    assert_eq!(result.ir.model.tessellations.len(), 1);
    let mesh = &result.ir.model.tessellations[0];
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
    assert_eq!(
        mesh.source_object
            .as_ref()
            .map(|source| source.object_id.as_str()),
        Some("fcstd:native:object#Mesh")
    );
    assert_eq!(result.ir.model.points.len(), 2);
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(11.0, 22.0, 33.0)
    );
    assert_eq!(
        result.ir.model.points[1].position,
        cadmpeg_ir::math::Point3::new(9.0, 18.0, 27.0)
    );
    assert!(result.report.geometry_transferred);
    assert!(result.report.losses.is_empty());
}

#[test]
fn retains_ordered_document_level_gui_state() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1" active="Perspective">
 <Camera orientation="0 0 0 1"><Position x="1" y="2" z="3"/></Camera>
 <ActiveView name="Perspective"/>
 <ClipPlane enabled="true" file="section.bin"/>
 <ViewProviderData Count="0"/>
</Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("section.bin", b"section-state"),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("GUI state");
    let namespace = result.ir.native.namespace("fcstd").expect("native");
    let documents = namespace
        .arena_as::<crate::native::GuiDocumentRecord>("gui_documents")
        .expect("GUI documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].schema_version, Some(1));
    assert_eq!(documents[0].attributes["active"], "Perspective");
    assert_eq!(
        documents[0]
            .states
            .iter()
            .map(|state| state.kind.as_str())
            .collect::<Vec<_>>(),
        ["Camera", "ActiveView", "ClipPlane"]
    );
    assert_eq!(documents[0].states[0].values[0].tag, "Position");
    assert_eq!(documents[0].states[2].side_entries, ["section.bin"]);
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let section = entries
        .iter()
        .find(|entry| entry.name == "section.bin")
        .expect("section asset");
    assert_eq!(section.referenced_by, [documents[0].states[2].id.clone()]);
    assert_eq!(result.ir.model.presentation_documents.len(), 1);
    let presentation = &result.ir.model.presentation_documents[0];
    assert_eq!(presentation.schema_version, Some(1));
    assert_eq!(presentation.active_view.as_deref(), Some("Perspective"));
    let camera = presentation.camera.as_ref().expect("camera state");
    assert_eq!(camera.position, Some([1.0, 2.0, 3.0]));
    assert_eq!(camera.orientation, Some([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(presentation.states[2].assets.len(), 1);
    assert!(presentation.states[2].assets[0].ends_with("section.bin"));
    assert!(result.ir.model.view_presentations.is_empty());
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);

    let mut corrupted = result.ir.clone();
    corrupted.model.presentation_documents[0]
        .camera
        .as_mut()
        .expect("camera state")
        .orientation = Some([0.0; 4]);
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid document presentation state"));
}

#[test]
fn gui_property_counts_ignore_nested_extension_properties() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="App::Feature" name="Model" id="1"/></Objects>
<ObjectData Count="1"><Object name="Model"><Properties Count="0"/></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Model"><Properties Count="0"/><Extension name="Nested"><Properties Count="1"><Property name="NestedValue" type="App::PropertyString"><String value="kept by extension"/></Property></Properties></Extension></ViewProvider>
</ViewProviderData></Document>"#;
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
        .ir
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
    assert!(crate::validate_native(&result.ir).is_empty());
}

#[test]
fn recovers_techdraw_page_template_and_view_graph() {
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
  <Property name="Scale" type="App::PropertyFloatConstraint"><Float value="2"/></Property>
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
        .ir
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
    assert_eq!(
        page.template.as_deref(),
        Some("fcstd:native:object#Template")
    );
    assert_eq!(page.views, ["fcstd:native:object#View"]);
    assert_eq!(template.side_entries, ["page.svg"]);
    assert_eq!(
        view.sources[0].object.as_deref(),
        Some("fcstd:native:object#Model")
    );
    assert!(view.parameters.contains_key("Direction"));
    assert_eq!(result.ir.model.drawings.len(), 3);
    let neutral_page = result
        .ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Page"))
        .expect("neutral page");
    let neutral_template = result
        .ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Template"))
        .expect("neutral template");
    let neutral_view = result
        .ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view");
    assert_eq!(neutral_page.kind, cadmpeg_ir::drawings::DrawingKind::Page);
    assert_eq!(
        neutral_page.template.as_deref(),
        Some(neutral_template.id.0.as_str())
    );
    assert_eq!(
        neutral_page.relationships["Views"][0].target.as_deref(),
        Some(neutral_view.id.0.as_str())
    );
    assert_eq!(neutral_template.assets.len(), 1);
    assert_eq!(neutral_view.position, Some([25.0, 40.0]));
    assert_eq!(neutral_view.scale, Some(2.0));
    assert_eq!(neutral_view.direction, Some([0.0, 0.0, 1.0]));
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);

    let mut corrupted = result.ir.clone();
    corrupted
        .model
        .drawings
        .iter_mut()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view")
        .scale = Some(0.0);
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid drawing reference, order, or numeric state"));
}

#[test]
fn separates_semantic_annotations_from_drawing_relationships() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawViewPart" name="View" id="2"/>
 <Object type="TechDraw::DrawViewDimension" name="Dimension" id="3"/>
 <Object type="TechDraw::DrawViewAnnotation" name="Note" id="4"/>
</Objects>
<ObjectData Count="4">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="View"><Properties Count="1"><Property name="Source" type="App::PropertyLink"><Link value="Model"/></Property></Properties></Object>
 <Object name="Dimension"><Properties Count="5">
  <Property name="BaseView" type="App::PropertyLink"><Link value="View"/></Property>
  <Property name="References2D" type="App::PropertyLinkSubList"><LinkList count="1"><Link obj="Model" sub="Edge1"/></LinkList></Property>
  <Property name="FormatSpec" type="App::PropertyString"><String value="12.5 mm"/></Property>
  <Property name="Measurement" type="App::PropertyLength"><Float value="12.5"/></Property>
  <Property name="Position" type="App::PropertyVector"><PropertyVector valueX="10" valueY="20" valueZ="0"/></Property>
 </Properties></Object>
 <Object name="Note"><Properties Count="2">
  <Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="INSPECT"/></StringList></Property>
  <Property name="View" type="App::PropertyLink"><Link value="View"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("semantic annotations");
    let namespace = result.ir.native.namespace("fcstd").expect("native");
    let annotations = namespace
        .arena_as::<crate::native::SemanticAnnotationRecord>("annotations")
        .expect("annotations");
    let drawings = namespace
        .arena_as::<crate::native::DrawingRecord>("drawings")
        .expect("drawings");
    assert_eq!(annotations.len(), 2);
    let dimension = annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Dimension"))
        .expect("dimension");
    assert_eq!(dimension.text, ["12.5 mm"]);
    assert_eq!(
        dimension.references["References2D"][0].subelements,
        ["Edge1"]
    );
    let note = annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .expect("note");
    assert_eq!(note.text, ["INSPECT"]);
    let drawing_dimension = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Dimension"))
        .expect("drawing dimension");
    assert_eq!(
        drawing_dimension.relationships["BaseView"][0]
            .object
            .as_deref(),
        Some("fcstd:native:object#View")
    );
    let neutral_dimension = result
        .ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Dimension"))
        .expect("neutral drawing dimension");
    assert_eq!(
        neutral_dimension.kind,
        cadmpeg_ir::drawings::DrawingKind::Dimension
    );
    assert!(neutral_dimension.relationships.contains_key("BaseView"));
    assert!(neutral_dimension.relationships.contains_key("References2D"));
    assert_eq!(result.ir.model.semantic_annotations.len(), 2);
    let semantic_dimension = result
        .ir
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Dimension"))
        .expect("semantic dimension");
    assert_eq!(
        semantic_dimension.kind,
        cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Dimension
    );
    assert_eq!(semantic_dimension.text, ["12.5 mm"]);
    assert_eq!(semantic_dimension.format.as_deref(), Some("12.5 mm"));
    assert_eq!(semantic_dimension.value, Some(12.5));
    assert_eq!(semantic_dimension.position, Some([10.0, 20.0, 0.0]));
    assert_eq!(
        semantic_dimension.references["References2D"][0].subelements,
        ["Edge1"]
    );
    let semantic_note = result
        .ir
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .expect("semantic note");
    assert_eq!(
        semantic_note.kind,
        cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text
    );
    assert_eq!(semantic_note.text, ["INSPECT"]);
    let neutral_view = result
        .ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#View"))
        .expect("neutral view");
    assert_eq!(
        semantic_note.references["View"][0].target.as_deref(),
        Some(neutral_view.id.0.as_str())
    );
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);

    let mut corrupted = result.ir.clone();
    corrupted.model.semantic_annotations[0].value = Some(f64::INFINITY);
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message
            == "invalid semantic annotation reference, order, or numeric state"));
}

#[test]
fn transfers_remaining_semantic_annotation_families_and_assets() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="Part::Feature" name="Model" id="1"/>
 <Object type="TechDraw::DrawViewBalloon" name="Balloon" id="2"/>
 <Object type="TechDraw::DrawLeaderLine" name="Leader" id="3"/>
 <Object type="TechDraw::DrawViewSymbol" name="Symbol" id="4"/>
 <Object type="TechDraw::DrawViewDatum" name="Datum" id="5"/>
 <Object type="TechDraw::DrawViewTolerance" name="Tolerance" id="6"/>
</Objects>
<ObjectData Count="6">
 <Object name="Model"><Properties Count="0"/></Object>
 <Object name="Balloon"><Properties Count="2">
  <Property name="Text" type="App::PropertyString"><String value="7"/></Property>
  <Property name="Source" type="App::PropertyLinkSub"><Link object="Model" sub="Face1"/></Property>
 </Properties></Object>
 <Object name="Leader"><Properties Count="1"><Property name="Text" type="App::PropertyString"><String value="LEAD"/></Property></Properties></Object>
 <Object name="Symbol"><Properties Count="1"><Property name="Symbol" type="App::PropertyFileIncluded"><FileIncluded file="symbol.svg"/></Property></Properties></Object>
 <Object name="Datum"><Properties Count="1"><Property name="LabelText" type="App::PropertyString"><String value="A"/></Property></Properties></Object>
 <Object name="Tolerance"><Properties Count="1"><Property name="Text" type="App::PropertyString"><String value="0.1"/></Property></Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("symbol.svg", b"<svg/>"),
            ])),
            &DecodeOptions::default(),
        )
        .expect("annotation families");
    use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind as Kind;
    let kinds = result
        .ir
        .model
        .semantic_annotations
        .iter()
        .map(|annotation| annotation.kind.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            Kind::Balloon,
            Kind::Datum,
            Kind::Leader,
            Kind::Symbol,
            Kind::GeometricTolerance
        ]
    );
    let balloon = &result.ir.model.semantic_annotations[0];
    assert_eq!(balloon.text, ["7"]);
    assert_eq!(balloon.references["Source"][0].subelements, ["Face1"]);
    let symbol = result
        .ir
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.kind == Kind::Symbol)
        .expect("semantic symbol");
    assert_eq!(symbol.assets.len(), 1);
    assert!(symbol.assets[0].ends_with("symbol.svg"));
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);
}

#[test]
fn transfers_non_default_extrusion_termination_branches() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="8">
  <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
  <Object type="PartDesign::Pad" name="ToLast" id="2"/>
  <Object type="PartDesign::Pad" name="ToFirst" id="3"/>
  <Object type="PartDesign::Pad" name="ToFace" id="4"/>
  <Object type="PartDesign::Pad" name="ToShape" id="5"/>
  <Object type="PartDesign::Pocket" name="ThroughAll" id="6"/>
  <Object type="PartDesign::Pad" name="Symmetric" id="7"/>
  <Object type="Part::Extrusion" name="PartExtrusion" id="8"/>
</Objects>
<ObjectData Count="8">
  <Object name="Sketch"><Properties Count="0"/></Object>
  <Object name="ToLast"><Properties Count="2">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  </Properties></Object>
  <Object name="ToFirst"><Properties Count="2">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  </Properties></Object>
  <Object name="ToFace"><Properties Count="3">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="3"/></Property>
    <Property name="UpToFace" type="App::PropertyLinkSub"><Link object="PartExtrusion" sub="Face1"/></Property>
  </Properties></Object>
  <Object name="ToShape"><Properties Count="3">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="5"/></Property>
    <Property name="UpToShape" type="App::PropertyLinkSub"><Link object="PartExtrusion" sub="Face2"/></Property>
  </Properties></Object>
  <Object name="ThroughAll"><Properties Count="2">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  </Properties></Object>
  <Object name="Symmetric"><Properties Count="5">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="12"/></Property>
    <Property name="Midplane" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="Reversed" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="TaperAngle" type="App::PropertyAngle"><Float value="5"/></Property>
  </Properties></Object>
  <Object name="PartExtrusion"><Properties Count="12">
    <Property name="Base" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Dir" type="App::PropertyVector"><Vector x="0" y="2" z="0"/></Property>
    <Property name="LengthFwd" type="App::PropertyLength"><Float value="7"/></Property>
    <Property name="LengthRev" type="App::PropertyLength"><Float value="3"/></Property>
    <Property name="TaperAngle" type="App::PropertyAngle"><Float value="2"/></Property>
    <Property name="TaperAngleRev" type="App::PropertyAngle"><Float value="4"/></Property>
    <Property name="DirMode" type="App::PropertyEnumeration"><Integer value="1"/></Property>
    <Property name="DirLink" type="App::PropertyLinkSub"><Link object="Sketch" sub="Edge1"/></Property>
    <Property name="Solid" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="FaceMakerClass" type="App::PropertyString"><String value="Part::FaceMakerUnified"/></Property>
    <Property name="FaceMakerMode" type="App::PropertyEnumeration"><Integer value="4"/></Property>
    <Property name="InnerWireTaper" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("extrusion termination branches");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    use cadmpeg_ir::features::{
        Angle, BooleanOp, Extent, ExtrusionDirectionSource, FeatureDefinition, InnerWireTaper,
        PathRef,
    };
    assert!(matches!(
        definition("ToLast"),
        FeatureDefinition::Extrude {
            extent: Extent::ToLast,
            ..
        }
    ));
    assert!(matches!(
        definition("ToFirst"),
        FeatureDefinition::Extrude {
            extent: Extent::ToFirst,
            ..
        }
    ));
    assert!(matches!(
        definition("ToFace"),
        FeatureDefinition::Extrude {
            extent: Extent::ToFace { .. },
            ..
        }
    ));
    assert!(matches!(
        definition("ToShape"),
        FeatureDefinition::Extrude {
            extent: Extent::ToShape { .. },
            ..
        }
    ));
    assert!(matches!(
        definition("ThroughAll"),
        FeatureDefinition::Extrude {
            extent: Extent::ThroughAll,
            op: BooleanOp::Cut,
            ..
        }
    ));
    assert!(matches!(
        definition("Symmetric"),
        FeatureDefinition::Extrude {
            direction: Some(direction),
            extent: Extent::Symmetric { length },
            draft: Some(Angle(draft)),
            ..
        } if direction.z == -1.0 && length.0 == 12.0 && (*draft - 5_f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        definition("PartExtrusion"),
        FeatureDefinition::Extrude {
            profile: _,
            direction: Some(direction),
            extent: Extent::TwoSided { first, second },
            draft: Some(Angle(draft)),
            reverse_draft: Some(Angle(reverse_draft)),
            direction_source: Some(ExtrusionDirectionSource::Edge { reference: PathRef::Native(reference) }),
            solid: Some(true),
            face_maker: Some(face_maker),
            inner_wire_taper: Some(InnerWireTaper::SameAsOuter),
            op: BooleanOp::NewBody,
            ..
        } if direction.y == 1.0 && first.0 == 7.0 && second.0 == 3.0
            && (*draft - 2_f64.to_radians()).abs() < 1e-12
            && (*reverse_draft - 4_f64.to_radians()).abs() < 1e-12
            && reference.ends_with(":DirLink")
            && face_maker.class == "Part::FaceMakerUnified" && face_maker.mode == Some(4)
    ));
}

#[test]
fn transfers_part_extrusion_symmetric_direction_magnitude() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
 <Object type="Part::Extrusion" name="Extrusion" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Profile"><Properties Count="0"/></Object>
 <Object name="Extrusion"><Properties Count="8">
  <Property name="Base" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Dir" type="App::PropertyVector"><Vector x="0" y="0" z="12"/></Property>
  <Property name="DirMode" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="LengthFwd" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="LengthRev" type="App::PropertyLength"><Float value="0"/></Property>
  <Property name="Symmetric" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Solid" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="TaperAngle" type="App::PropertyAngle"><Float value="3"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("symmetric Part extrusion");
    let definition = &result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrusion"))
        .expect("extrusion")
        .definition;
    assert!(matches!(
        definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::Extent::Symmetric { length },
            direction_source: Some(cadmpeg_ir::features::ExtrusionDirectionSource::ProfileNormal),
            solid: Some(false),
            draft: Some(cadmpeg_ir::features::Angle(draft)),
            reverse_draft: Some(cadmpeg_ir::features::Angle(reverse_draft)),
            ..
        } if length.0 == 12.0
            && (*draft - 3_f64.to_radians()).abs() < 1e-12
            && (*reverse_draft - 3_f64.to_radians()).abs() < 1e-12
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_partdesign_mixed_extrusion_side_controls() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="5">
 <Object type="Sketcher::SketchObject" name="Profile" id="1"/>
 <Object type="Part::Box" name="Target" id="2"/>
 <Object type="PartDesign::Pad" name="Mixed" id="3"/>
 <Object type="PartDesign::Pocket" name="Symmetric" id="4"/>
 <Object type="PartDesign::Pad" name="LegacyTwoLengths" id="5"/>
</Objects>
<ObjectData Count="5">
 <Object name="Profile"><Properties Count="0"/></Object>
 <Object name="Target"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Mixed"><Properties Count="15">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="SideType" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="-5"/></Property>
  <Property name="Type2" type="App::PropertyEnumeration"><Integer value="5"/></Property>
  <Property name="UpToShape2" type="App::PropertyLinkSubList"><LinkList count="1"><Link object="Target" sub="Face2"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyVector"><Vector x="0" y="3" z="0"/></Property>
  <Property name="UseCustomVector" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="ReferenceAxis" type="App::PropertyLinkSub"><Link object="Profile" sub="Edge1"/></Property>
  <Property name="TaperAngle" type="App::PropertyAngle"><Float value="2"/></Property>
  <Property name="TaperAngle2" type="App::PropertyAngle"><Float value="-3"/></Property>
  <Property name="Offset" type="App::PropertyDistance"><Float value="1"/></Property>
  <Property name="Offset2" type="App::PropertyDistance"><Float value="-2"/></Property>
  <Property name="AlongSketchNormal" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="AllowMultiFace" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Symmetric"><Properties Count="4">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="SideType" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="1"/></Property>
  <Property name="Offset" type="App::PropertyDistance"><Float value="0.5"/></Property>
 </Properties></Object>
 <Object name="LegacyTwoLengths"><Properties Count="4">
  <Property name="Profile" type="App::PropertyLink"><Link value="Profile"/></Property>
  <Property name="Type" type="App::PropertyEnumeration"><Integer value="4"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="6"/></Property>
  <Property name="Length2" type="App::PropertyLength"><Float value="2"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("mixed extrusion controls");
    let definition = |name: &str| {
        &result
            .ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .definition
    };
    use cadmpeg_ir::features::{
        Angle, Extent, ExtrusionDirectionSource, FeatureDefinition, Length, PathRef,
    };
    assert!(matches!(
        definition("Mixed"),
        FeatureDefinition::Extrude {
            extent: Extent::TwoSidedExtents { first, second },
            direction: Some(direction),
            direction_source: Some(ExtrusionDirectionSource::Edge { reference: PathRef::Native(reference) }),
            draft: Some(Angle(first_draft)),
            reverse_draft: Some(Angle(second_draft)),
            first_offset: Some(Length(1.0)),
            second_offset: Some(Length(-2.0)),
            length_along_profile_normal: Some(false),
            allow_multi_profile_faces: Some(true),
            ..
        } if matches!(first.as_ref(), Extent::Blind { length: Length(-5.0) })
            && matches!(second.as_ref(), Extent::ToShape { .. })
            && direction.y == 1.0
            && reference.ends_with(":ReferenceAxis")
            && (*first_draft - 2_f64.to_radians()).abs() < 1e-12
            && (*second_draft + 3_f64.to_radians()).abs() < 1e-12
    ));
    assert!(matches!(
        definition("Symmetric"),
        FeatureDefinition::Extrude {
            extent: Extent::SymmetricExtent { extent },
            first_offset: Some(Length(0.5)),
            ..
        } if matches!(extent.as_ref(), Extent::ThroughAll)
    ));
    assert!(matches!(
        definition("LegacyTwoLengths"),
        FeatureDefinition::Extrude {
            extent: Extent::TwoSidedExtents { first, second },
            ..
        } if matches!(first.as_ref(), Extent::Blind { length: Length(6.0) })
            && matches!(second.as_ref(), Extent::Blind { length: Length(2.0) })
    ));
    assert!(result.report.losses.is_empty());
}

#[test]
fn transfers_sketch_pad_and_pocket_design_history() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4">
  <Object type="PartDesign::Body" name="Body" id="1"/>
  <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
  <Object type="PartDesign::Pad" name="Pad" id="2"/>
  <Object type="PartDesign::Pocket" name="Pocket" id="3"/>
  <ObjectDeps Name="Pad"><Dep Name="Sketch"/></ObjectDeps>
  <ObjectDeps Name="Pocket"><Dep Name="Pad"/><Dep Name="Sketch"/></ObjectDeps>
</Objects>
<ObjectData Count="4">
  <Object name="Body"><Properties Count="2">
    <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="Sketch"/><Link value="Pad"/><Link value="Pocket"/></LinkList></Property>
    <Property name="Tip" type="App::PropertyLink"><Link value="Pocket"/></Property>
  </Properties></Object>
  <Object name="Sketch"><Properties Count="3">
    <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="4">
      <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="10" EndY="0"/><Construction value="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><LineSegment StartX="10" StartY="0" EndX="10" EndY="5"/><Construction value="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><LineSegment StartX="10" StartY="5" EndX="0" EndY="5"/><Construction value="0"/></Geometry>
      <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="5" EndX="0" EndY="0"/><Construction value="0"/></Geometry>
    </GeometryList></Property>
    <Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="2">
      <Constrain Type="2" First="0" FirstPos="0"/>
      <Constrain Name="Width" Type="7" Value="10" IsDriving="1" First="0" FirstPos="1" Second="1" SecondPos="1"/>
    </ConstraintList></Property>
    <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0.7071067811865476" Q1="0" Q2="0" Q3="0.7071067811865476"/></Property>
  </Properties></Object>
  <Object name="Pad"><Properties Count="2">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="10"/></Property>
  </Properties></Object>
  <Object name="Pocket"><Properties Count="4">
    <Property name="Profile" type="App::PropertyLink"><Link value="Sketch"/></Property>
    <Property name="Length" type="App::PropertyLength"><Float value="2.5"/></Property>
    <Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/></Property>
    <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Length" expression="Pad.Length / 4"/></ExpressionEngine></Property>
  </Properties></Object>
</ObjectData>
</Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("design history");
    assert_eq!(result.ir.model.sketches.len(), 1);
    assert_eq!(result.ir.model.sketch_entities.len(), 4);
    assert_eq!(result.ir.model.sketches[0].profiles.len(), 1);
    assert_eq!(result.ir.model.sketches[0].profiles[0].len(), 4);
    let (origin, normal, _) = result.ir.model.sketches[0]
        .resolved_placement()
        .expect("resolved sketch placement");
    assert_eq!(origin.x, 1.0);
    assert!((normal.y + 1.0).abs() < 1e-12);
    assert_eq!(result.ir.model.features.len(), 4);
    assert_eq!(result.ir.model.sketch_constraints.len(), 2);
    assert_eq!(result.ir.model.parameters.len(), 3);
    assert!(result.ir.model.sketch_constraints.iter().any(|constraint| {
        matches!(
            constraint.definition,
            cadmpeg_ir::sketches::SketchConstraintDefinition::Horizontal { .. }
        )
    }));
    assert!(result.ir.model.sketch_constraints.iter().any(|constraint| {
        matches!(
            constraint.definition,
            cadmpeg_ir::sketches::SketchConstraintDefinition::HorizontalDistance { .. }
        )
    }));
    let pad = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .expect("pad");
    let pocket = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pocket"))
        .expect("pocket");
    let body = result
        .ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body"))
        .expect("body");
    assert_eq!(pad.parent.as_ref(), Some(&body.id));
    assert_eq!(pocket.parent.as_ref(), Some(&body.id));
    assert_eq!(
        body.source_properties.get("Tip").map(String::as_str),
        Some("fcstd:native:object#Pocket")
    );
    assert_eq!(pocket.suppressed, Some(true));
    assert_eq!(
        pocket
            .source_properties
            .get("Suppressed")
            .map(String::as_str),
        Some("true")
    );
    let pocket_length = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner == pocket.id && parameter.name == "Length")
        .expect("pocket length");
    assert_eq!(pocket_length.expression, "Pad.Length / 4");
    assert_eq!(pocket_length.dependencies.len(), 1);
    assert!(matches!(
        pad.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Sketch(_),
            extent: cadmpeg_ir::features::Extent::Blind {
                length: cadmpeg_ir::features::Length(10.0)
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }
    ));
    assert!(matches!(
        pocket.definition,
        cadmpeg_ir::features::FeatureDefinition::Extrude {
            extent: cadmpeg_ir::features::Extent::Blind {
                length: cadmpeg_ir::features::Length(2.5)
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }
    ));
    let native_findings = crate::validate_native(&result.ir);
    assert!(native_findings.is_empty(), "{native_findings:#?}");
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    let design_findings = validation
        .findings
        .iter()
        .filter(|finding| {
            finding
                .entity
                .as_deref()
                .is_some_and(|entity| entity.starts_with("fcstd:design:"))
        })
        .collect::<Vec<_>>();
    assert!(design_findings.is_empty(), "{design_findings:#?}");
}

#[test]
fn retains_support_attachment_and_distinct_offset_frame() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="2">
 <Object type="PartDesign::Feature" name="Support" id="1"/>
 <Object type="Sketcher::SketchObject" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
 <Object name="Support"><Properties Count="0"/></Object>
 <Object name="Sketch"><Properties Count="5">
  <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="0"/></Property>
  <Property name="Support" type="App::PropertyLinkSub"><Link object="Support" sub="Face1"/></Property>
  <Property name="MapMode" type="App::PropertyString"><String value="FlatFace"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="10" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="AttachmentOffset" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive(document)),
            &DecodeOptions::default(),
        )
        .expect("attachment");
    let namespace = result.ir.native.namespace("fcstd").expect("native");
    let attachments = namespace
        .arena_as::<crate::native::AttachmentRecord>("attachments")
        .expect("attachments");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].map_mode.as_deref(), Some("FlatFace"));
    assert_eq!(
        attachments[0].supports[0].object.as_deref(),
        Some("fcstd:native:object#Support")
    );
    assert_eq!(attachments[0].supports[0].subelements, ["Face1"]);
    assert_eq!(attachments[0].placement.expect("placement")[0][3], 10.0);
    assert_eq!(attachments[0].offset.expect("offset")[0][3], 2.0);
    assert_eq!(attachments[0].effective_frame[0][3], 10.0);
    let sketch = result.ir.model.sketches.first().expect("sketch");
    assert_eq!(
        sketch
            .resolved_placement()
            .expect("resolved sketch placement")
            .0
            .x,
        10.0
    );
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);
}

#[test]
fn transfers_recursive_exact_parameter_curve_geometry() {
    let source = crate::brep::TextCurve2d::Offset {
        distance: 0.25,
        basis: Box::new(crate::brep::TextCurve2d::Trimmed {
            parameter_range: [0.0, std::f64::consts::PI],
            basis: Box::new(crate::brep::TextCurve2d::Circle {
                center: cadmpeg_ir::math::Point2::new(1.0, 2.0),
                x_axis: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                y_axis: cadmpeg_ir::math::Point2::new(0.0, 1.0),
                radius: 3.0,
            }),
        }),
    };
    let cadmpeg_ir::geometry::PcurveGeometry::Offset { distance, basis } =
        crate::topology_transfer::pcurve_geometry(&source)
    else {
        panic!("expected offset pcurve");
    };
    assert_eq!(distance, 0.25);
    assert!(matches!(
        basis.as_ref(),
        cadmpeg_ir::geometry::PcurveGeometry::Trimmed { basis, .. }
            if matches!(basis.as_ref(), cadmpeg_ir::geometry::PcurveGeometry::Circle { radius: 3.0, .. })
    ));
}

#[test]
fn transfers_binary_exact_curve_and_surface_carriers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.bin"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let mut brep = b"\nOpen CASCADE Topology V3 (c)\nLocations 0\nCurve2ds 0\nCurves 1\n".to_vec();
    brep.push(1);
    for value in [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.extend_from_slice(b"Polygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n");
    brep.push(1);
    for value in [
        0.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
    ] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.extend_from_slice(b"Triangulations 1\n");
    brep.extend_from_slice(&3_i32.to_le_bytes());
    brep.extend_from_slice(&1_i32.to_le_bytes());
    brep.push(0);
    for value in [0.01_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    for node in [1_i32, 2, 3] {
        brep.extend_from_slice(&node.to_le_bytes());
    }
    brep.extend_from_slice(b"TShapes 7\n");
    let flags = |brep: &mut Vec<u8>| brep.extend_from_slice(&[1, 0, 0, 1, 0, 0, 0]);
    let child = |brep: &mut Vec<u8>, orientation: u8, reverse_index: i32| {
        brep.push(orientation);
        brep.extend_from_slice(&reverse_index.to_le_bytes());
        brep.extend_from_slice(&0_i32.to_le_bytes());
    };
    brep.push(7);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    for value in [0.0_f64, 0.0, 0.0] {
        brep.extend_from_slice(&value.to_le_bytes());
    }
    brep.push(0);
    flags(&mut brep);
    brep.push(b'*');
    brep.push(6);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    brep.extend_from_slice(&[1, 1, 1, 0]);
    flags(&mut brep);
    child(&mut brep, 0, 7);
    child(&mut brep, 1, 7);
    brep.push(b'*');
    brep.push(5);
    flags(&mut brep);
    child(&mut brep, 0, 6);
    brep.push(b'*');
    brep.push(4);
    brep.push(0);
    brep.extend_from_slice(&0.001_f64.to_le_bytes());
    brep.extend_from_slice(&1_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    brep.push(2);
    brep.extend_from_slice(&1_i32.to_le_bytes());
    flags(&mut brep);
    child(&mut brep, 0, 5);
    brep.push(b'*');
    for (kind, reverse_index) in [(3_u8, 4_i32), (2, 3), (0, 2)] {
        brep.push(kind);
        flags(&mut brep);
        child(&mut brep, 0, reverse_index);
        brep.push(b'*');
    }
    brep.extend_from_slice(&7_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    brep.extend_from_slice(&0_i32.to_le_bytes());
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.bin", &brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("binary curve carrier");
    assert_eq!(result.ir.model.curves.len(), 1);
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Line { .. }
    ));
    assert_eq!(result.ir.model.surfaces.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
    ));
    assert_eq!(result.ir.model.tessellations.len(), 1);
    assert_eq!(result.ir.model.tessellations[0].triangles, [[0, 1, 2]]);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(
        result.ir.model.tessellations[0].body.as_ref(),
        Some(&result.ir.model.bodies[0].id)
    );
    assert_eq!(
        result.ir.model.tessellations[0].faces,
        [result.ir.model.faces[0].id.clone()]
    );
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(result.report.geometry_transferred);
}

fn archive(document: &str) -> Vec<u8> {
    archive_entries(&[("Document.xml", document.as_bytes())])
}

fn archive_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut bytes);
    for (name, data) in entries {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .expect("start entry");
        zip.write_all(data).expect("write entry");
    }
    zip.finish().expect("finish ZIP");
    bytes.into_inner()
}

fn streaming_archive(document: &str) -> Vec<u8> {
    streaming_archive_with_options(document, SimpleFileOptions::default())
}

fn streaming_archive_with_options(document: &str, options: SimpleFileOptions) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_stream(Vec::new());
    zip.start_file(
        "Document.xml",
        options.compression_method(zip::CompressionMethod::Deflated),
    )
    .expect("start streamed entry");
    zip.write_all(document.as_bytes()).expect("write entry");
    zip.finish().expect("finish ZIP").into_inner()
}

#[test]
fn frames_zip64_streaming_descriptor_and_local_extra() {
    let bytes = streaming_archive_with_options(
        "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>",
        SimpleFileOptions::default().large_file(true),
    );
    let scan = crate::container::scan(&mut Cursor::new(bytes)).expect("ZIP64 streaming ZIP");
    assert!(scan
        .ledger
        .iter()
        .any(|span| span.role == "local-extra" && span.end > span.start));
    let descriptor = scan
        .ledger
        .iter()
        .find(|span| span.role == "data-descriptor")
        .expect("ZIP64 descriptor");
    assert_eq!(descriptor.end - descriptor.start, 24);
}

#[test]
fn frames_streaming_data_descriptor_separately_from_padding() {
    let bytes = streaming_archive("<Document SchemaVersion=\"4\" FileVersion=\"1\"/>");
    let scan = crate::container::scan(&mut Cursor::new(bytes)).expect("streaming ZIP");
    let descriptors = scan
        .ledger
        .iter()
        .filter(|span| span.role == "data-descriptor")
        .collect::<Vec<_>>();
    assert_eq!(descriptors.len(), 1);
    assert!(matches!(descriptors[0].end - descriptors[0].start, 16 | 24));
}

#[test]
fn rejects_unsafe_names() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let unsafe_name = archive_entries(&[("../Document.xml", xml), ("Document.xml", xml)]);
    let error = FcstdCodec
        .inspect(&mut Cursor::new(unsafe_name))
        .expect_err("unsafe path must fail");
    assert!(error.to_string().contains("unsafe ZIP entry path"));
}

#[test]
fn transfers_connected_text_brep_topology() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1">
<ViewProvider name="Shape" expanded="1"><Properties Count="7">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3368601600"/></Property>
<Property name="LineColor" type="App::PropertyColor"><PropertyColor value="4278190335"/></Property>
<Property name="LineWidth" type="App::PropertyFloatConstraint"><Float value="2.5"/></Property>
<Property name="PointColor" type="App::PropertyColor"><PropertyColor value="16711935"/></Property>
<Property name="PointSize" type="App::PropertyFloatConstraint"><Float value="4"/></Property>
<Property name="Transparency" type="App::PropertyPercent"><Integer value="25"/></Property>
<Property name="Visibility" type="App::PropertyBool"><Bool value="false"/></Property>
</Properties></ViewProvider></ViewProviderData></Document>"#;
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
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("Shape.brp", brep),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("connected topology");
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 2);
    assert_eq!(result.ir.model.edges.len(), 2);
    assert_eq!(result.ir.model.vertices.len(), 2);
    assert_eq!(result.ir.model.pcurves.len(), 2);
    assert_eq!(result.ir.model.appearances.len(), 3);
    assert_eq!(result.ir.model.appearance_bindings.len(), 5);
    assert_eq!(
        result
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
            ))
            .count(),
        2
    );
    assert_eq!(
        result
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(
                binding.target,
                cadmpeg_ir::appearance::AppearanceTarget::Vertex(_)
            ))
            .count(),
        2
    );
    assert_eq!(
        result
            .ir
            .model
            .appearances
            .iter()
            .find(|appearance| appearance.schema.as_deref() == Some("FCStd ViewProvider line style"))
            .and_then(|appearance| appearance.properties.get("line_width")),
        Some(&2.5)
    );
    assert_eq!(
        result
            .ir
            .model
            .appearances
            .iter()
            .find(
                |appearance| appearance.schema.as_deref() == Some("FCStd ViewProvider point style")
            )
            .and_then(|appearance| appearance.properties.get("point_size")),
        Some(&4.0)
    );
    assert_eq!(result.ir.model.bodies[0].visible, Some(false));
    assert_eq!(result.ir.model.presentation_documents.len(), 1);
    assert_eq!(result.ir.model.view_presentations.len(), 1);
    let view = &result.ir.model.view_presentations[0];
    assert!(view
        .object
        .as_deref()
        .is_some_and(|id| id.ends_with("Shape")));
    assert_eq!(view.order, 0);
    assert_eq!(view.expanded, Some(true));
    assert_eq!(view.visible, Some(false));
    assert_eq!(view.line_width, Some(2.5));
    assert_eq!(view.point_size, Some(4.0));
    let color = result.ir.model.bodies[0].color.expect("shape color");
    assert!((color.r - 200.0 / 255.0).abs() < 1e-6);
    assert!((color.a - 0.75).abs() < 1e-6);
    let namespace = result.ir.native.namespace("fcstd").expect("native");
    assert_eq!(namespace.version, 20);
    let census = namespace
        .arena_as::<crate::native::CarrierCensusRecord>("carrier_census")
        .expect("carrier census");
    assert_eq!(census.len(), 1);
    assert_eq!(census[0].topology_version, 1);
    assert_eq!(census[0].curves_2d["line"], 2);
    assert_eq!(census[0].curves_3d["line"], 2);
    assert_eq!(census[0].surfaces["plane"], 1);
    assert_eq!(census[0].topology["edge"], 2);
    assert_eq!(census[0].topology["vertex"], 2);
    let gui_providers = namespace
        .arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")
        .expect("GUI providers");
    let gui_properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(gui_providers.len(), 1);
    assert_eq!(
        gui_providers[0].object.as_deref(),
        Some("fcstd:native:object#Shape")
    );
    assert_eq!(gui_properties.len(), 7);
    assert!(gui_properties
        .iter()
        .all(|property| property.raw_xml.starts_with("<Property")));
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);

    let mut corrupted = result.ir.clone();
    corrupted.model.view_presentations[0].line_width = Some(f64::NAN);
    assert!(cadmpeg_ir::validate(&corrupted, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "invalid view presentation reference, order, or size"));
    assert!(result
        .ir
        .model
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty()));
    let report = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::Severity::Error),
        "{:#?}",
        report.findings
    );
}

#[test]
fn transfers_triangulation_only_face_and_indexed_edge_polygon() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="MeshShape" id="1"/></Objects>
<ObjectData Count="1"><Object name="MeshShape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V3, (c) Open Cascade
Locations 1
1 1 0 0 10 0 1 0 0 0 0 1 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 1
2 1 2 p 0.01 1 0 1
Surfaces 0
Triangulations 1
3 1 0 0 0.02 0 0 0 1 0 0 0 1 0 1 2 3
TShapes 7
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 6 1 1 0 0 1001000 +7 0 -6 0 *
Wi 1001000 +5 0 *
Fa 0 0.001 0 1 2 1 1001000 +4 0 *
Sh 1001000 +3 0 *
So 1001000 +2 0 *
+1 0 *";
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(archive_entries(&[
                ("Document.xml", document.as_bytes()),
                ("Shape.brp", brep),
            ])),
            &DecodeOptions::default(),
        )
        .expect("triangulation-only topology");
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.tessellations.len(), 1);
    assert_eq!(result.ir.model.tessellations[0].vertices[0].x, 0.0);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Polygonal {
            chordal_deflection: 0.02,
            ..
        }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        cadmpeg_ir::geometry::CurveGeometry::Polyline {
            chordal_deflection: 0.01,
            ..
        }
    ));
    assert_eq!(result.ir.model.edges[0].param_range, Some([0.0, 1.0]));
    assert!(result.report.losses.is_empty());
    let validation = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(
        validation.findings.iter().all(|finding| {
            finding.severity < cadmpeg_ir::Severity::Error
                || finding.check == cadmpeg_ir::Check::Identity
        }),
        "{:#?}",
        validation.findings
    );
}

#[test]
fn connects_persistent_element_names_to_neutral_topology() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1" StringHasher="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape">
<Part HasherIndex="0" SaveHasher="1" ElementMap="1.0" file="Shape.brp"/>
<StringHasher saveall="0" threshold="16" count="0" new="1"/>
<StringHasher2 count="1">
a.c PersistentSource
</StringHasher2>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="5">
41 PostfixCount 0 MapCount 1
ElementMap 1 41 3
Face ChildCount 0 NameCount 1
;FaceStable.0.a 0
Edge ChildCount 0 NameCount 2
;EdgeStable1.0.a 0
;EdgeStable2.0.a 0
Vertex ChildCount 0 NameCount 2
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2>
</Property></Properties></Object></ObjectData>
</Document>"#;
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Shape"><Properties Count="4">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3435973632"/></Property>
<Property name="DiffuseColor" type="App::PropertyColorList"><ColorList file="DiffuseColor"/></Property>
<Property name="LineColorArray" type="App::PropertyColorList"><ColorList file="LineColorArray"/></Property>
<Property name="PointColorArray" type="App::PropertyColorList"><ColorList file="PointColorArray"/></Property>
</Properties></ViewProvider></ViewProviderData></Document>"#;
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
    let face_colors = [1_u8, 0, 0, 0, 0, 0, 0, 255];
    let edge_colors = [2_u8, 0, 0, 0, 255, 0, 0, 255, 0, 255, 0, 255];
    let point_colors = [2_u8, 0, 0, 0, 0, 0, 255, 255, 255, 255, 0, 255];
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("DiffuseColor", &face_colors),
        ("LineColorArray", &edge_colors),
        ("PointColorArray", &point_colors),
        ("Shape.brp", brep),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("persistent element map");
    let namespace = result.ir.native.namespace("fcstd").unwrap();
    let tables = namespace
        .arena_as::<crate::native::StringTableRecord>("string_tables")
        .unwrap();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].entries[0].string_id, 10);
    let maps = namespace
        .arena_as::<crate::native::ElementMapRecord>("element_maps")
        .unwrap();
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].hasher_index, Some(0));
    let groups = &maps[0].maps[0].groups;
    assert_eq!(groups[0].names[0][0].topology_ids.len(), 1);
    assert_eq!(groups[1].names[0][0].topology_ids.len(), 1);
    assert_eq!(groups[1].names[1][0].topology_ids.len(), 1);
    assert_eq!(groups[2].names[0][0].topology_ids.len(), 1);
    assert_eq!(groups[2].names[1][0].topology_ids.len(), 1);
    assert!(result.ir.model.appearance_bindings.iter().any(|binding| {
        matches!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Face(_)
        ) && binding.channels.get("precedence").map(String::as_str) == Some("face_over_object")
    }));
    assert_eq!(
        result
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Edge(_)
                ) && binding.channels.get("precedence").map(String::as_str)
                    == Some("edge_array_over_line")
            })
            .count(),
        2
    );
    assert_eq!(
        result
            .ir
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.target,
                    cadmpeg_ir::appearance::AppearanceTarget::Vertex(_)
                ) && binding.channels.get("precedence").map(String::as_str)
                    == Some("vertex_array_over_point")
            })
            .count(),
        2
    );
    assert!(crate::validate_native(&result.ir).is_empty());
    assert_valid_document(&result.ir);
}

#[test]
fn binds_both_seam_pcurves_and_closes_the_radial_pair() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 2
1 0 0 0 1
1 6.283185307179586 0 0 1
Curves 1
1 1 0 0 0 0 1
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
2 0 0 0 0 0 1 1 0 0 0 1 0 1
Triangulations 0
TShapes 8
Ve 0.001 1 0 0 0 0 1001000 *
Ve 0.001 1 0 1 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 3 1 2 C0 1 0 0 1 0 1001000 +8 0 -7 0 *
Wi 1001000 +6 0 -6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 0 *
So 1001000 +3 0 *
Co 1001000 +2 0 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("cylindrical seam");
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 2);
    let first = &result.ir.model.coedges[0];
    let second = &result.ir.model.coedges[1];
    assert_eq!(first.radial_next, second.id);
    assert_eq!(second.radial_next, first.id);
    assert_ne!(first.pcurves, second.pcurves);
    assert!(!first.pcurves.is_empty() && !second.pcurves.is_empty());
    let errors = cadmpeg_ir::validate(&result.ir, Vec::new())
        .findings
        .into_iter()
        .filter(|finding| finding.severity == cadmpeg_ir::Severity::Error)
        .filter(|finding| finding.check != cadmpeg_ir::Check::Identity)
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn preserves_a_free_edge_as_a_wire_body() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
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
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("free edge");
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.shells[0].wire_edges.len(), 1);
    assert!(result.ir.model.shells[0].faces.is_empty());
}

#[test]
fn preserves_an_unbounded_edge_as_a_free_exact_curve() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::Line" name="Axis" id="1"/></Objects>
<ObjectData Count="1"><Object name="Axis"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Axis.brp"/></Property></Properties></Object></ObjectData>
</Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 0 0 1
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 1
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Axis.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("unbounded datum axis");
    assert!(result.ir.model.bodies.is_empty());
    assert_eq!(result.ir.model.curves.len(), 1);
    assert!(result.ir.model.curves[0].source_object.is_some());
    assert_valid_document(&result.ir);
}

#[test]
fn preserves_compound_ownership_and_composes_nested_mirrored_locations_once() {
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
TShapes 9
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 2 1 1 0 0 1 0 1001000 +9 0 -8 0 *
Ed 0.001 1 1 0 1 2 0 0 1 2 2 1 0 0 1 0 1001000 +8 0 -9 0 *
Wi 1001000 +7 0 +6 0 *
Fa 0 0.001 1 0 1001000 +5 0 *
Sh 1001000 +4 2 *
So 1001000 +3 0 *
Co 1001000 +2 1 +2 3 *
+1 0 *";
    let bytes = archive_entries(&[("Document.xml", document.as_bytes()), ("Shape.brp", brep)]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("located topology");
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(result.ir.model.bodies[0].regions.len(), 2);
    assert!(result.ir.model.bodies[0].transform.is_none());
    assert_eq!(result.ir.model.edges.len(), 4);
    assert_eq!(result.ir.model.vertices.len(), 4);
    let mut positions = result
        .ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .map(|vertex| {
            let vertex = result
                .ir
                .model
                .vertices
                .iter()
                .find(|candidate| &candidate.id == vertex)
                .unwrap();
            result
                .ir
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .unwrap()
                .position
        })
        .collect::<Vec<_>>();
    positions.sort_by(|left, right| left.x.total_cmp(&right.x));
    positions.dedup();
    assert_eq!(positions.len(), 4);
    assert_eq!([positions[0].x, positions[0].y], [8.0, 5.0]);
    assert_eq!([positions[1].x, positions[1].y], [10.0, 5.0]);
    assert_eq!([positions[2].x, positions[2].y], [18.0, 5.0]);
    assert_eq!([positions[3].x, positions[3].y], [20.0, 5.0]);
    let face = &result.ir.model.faces[0];
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap();
    let cadmpeg_ir::geometry::SurfaceGeometry::Transformed { basis, transform } = &surface.geometry
    else {
        panic!("located face must retain its exact transformed basis");
    };
    assert!(matches!(
        basis.as_ref(),
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
    ));
    assert_eq!(transform.rows[0][0], -2.0);
    assert_eq!(transform.rows[1][1], 2.0);
    let origin = cadmpeg_ir::eval::surface_point(&surface.geometry, 0.0, 0.0).unwrap();
    assert_eq!([origin.x, origin.y], [10.0, 5.0]);
    for edge in &result.ir.model.edges {
        let curve = result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve.as_ref())
            .unwrap();
        let range = edge.param_range.expect("located edge parameter range");
        let start = cadmpeg_ir::eval::curve_point(&curve.geometry, range[0]).unwrap();
        let end = cadmpeg_ir::eval::curve_point(&curve.geometry, range[1]).unwrap();
        assert_eq!((start.x - end.x).abs(), 2.0);
    }
    let report = cadmpeg_ir::validate(&result.ir, Vec::new());
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.severity < cadmpeg_ir::Severity::Error
                || finding.check == cadmpeg_ir::Check::Identity),
        "{:#?}",
        report.findings
    );
}

#[test]
fn legacy_layout_is_inspectable_but_explicitly_refused_for_decode() {
    let bytes = archive("<Document SchemaVersion=\"3\" FileVersion=\"1\"/>");
    let summary = FcstdCodec
        .inspect(&mut Cursor::new(&bytes))
        .expect("legacy inspection");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=3"));
    let error = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect_err("legacy decode must fail");
    assert!(error
        .to_string()
        .contains("FCStd SchemaVersion=3 FileVersion=1 persistence layout"));
}

#[test]
fn thumbnail_bytes_are_retained_with_digest() {
    let xml = b"<Document SchemaVersion=\"4\" FileVersion=\"1\"/>";
    let bytes = archive_entries(&[("Document.xml", xml), ("thumbnails/Thumbnail.png", b"png")]);
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
            },
        )
        .expect("decode");
    let unknowns = result
        .source_fidelity
        .native_unknown_records(&result.ir, "fcstd")
        .expect("unknowns");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].data.as_deref(), Some(b"png".as_slice()));
}

#[test]
fn recovers_objects_dynamic_properties_links_and_side_entries() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Demo"/></Property></Properties>
<Objects Count="2" Dependencies="1">
<ObjectDeps Name="Body" Count="1"><Dep Name="Sketch"/></ObjectDeps>
<ObjectDeps Name="Sketch" Count="0"/>
<Object type="PartDesign::Body" name="Body" id="1" Touched="1"/>
<Object type="PartDesign::Feature" name="Sketch" id="2"/>
</Objects>
<ObjectData Count="2">
<Object name="Body" Extensions="True"><Extensions Count="1"><Extension type="Demo::Extension" name="Demo"><Properties Count="1"><Property name="ExtensionValue" type="App::PropertyString"><String value="kept"/></Property></Properties></Extension></Extensions><Properties Count="4" TransientCount="1">
<_Property name="TransientState" type="App::PropertyInteger" status="8"/>
<Property name="Support" type="App::PropertyLinkSub" status="4" group="Attachment" doc="Support object" attr="2" ro="1" hide="0"><Link object="Sketch" sub="Face1"/></Property>
<Property name="Members" type="App::PropertyLinkList"><LinkList count="2"><Link value="Sketch"/><Link value=""/></LinkList></Property>
<Property name="Payload" type="App::PropertyFileIncluded"><File file="Payload.bin"/></Property>
<Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="" file="Shape.brp"/></Property>
</Properties></Object>
<Object name="Sketch"><Properties Count="0"></Properties></Object>
</ObjectData></Document>"#;
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Payload.bin", b"payload"),
        (
            "Shape.brp",
            b"\nCASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 4\n1 10 20 30 1 0 0\n7 0 0 2 3 2 0 0 0 5 0 0 10 0 0 0 3 1 3\n8 0 5 1 0 0 0 1 0 0\n9 2 0 0 1 1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 5\n1 0 0 0 0 0 1 1 0 0 0 1 0\n9 0 0 0 0 1 1 2 2 2 2 0 0 0 0 1 0 1 0 0 1 1 0 0 2 1 2 0 2 1 2\n6 0 0 2 1 0 0 0 1 0 0\n7 0 0 0 0 0 1 1 0 0 0 1 0 0\n10 0 1 2 3 11 4 1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 1\n3 1 0 0.01 0 0 0 1 0 0 0 1 0 1 2 3\nTShapes 0\n*",
        ),
    ]);
    let result = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode graph");
    let namespace = result.ir.native.namespace("fcstd").expect("namespace");
    let objects = namespace
        .arena_as::<crate::native::ObjectRecord>("objects")
        .expect("objects");
    let properties = namespace
        .arena_as::<crate::native::PropertyRecord>("properties")
        .expect("properties");
    let extensions = namespace
        .arena_as::<crate::native::ExtensionRecord>("extensions")
        .expect("extensions");
    assert_eq!(objects.len(), 2);
    assert_eq!(extensions.len(), 1);
    assert_eq!(extensions[0].owner, "fcstd:native:object#Body");
    let extension_value = properties
        .iter()
        .find(|property| property.name == "ExtensionValue")
        .expect("extension property");
    assert_eq!(extension_value.owner, extensions[0].id);
    assert_eq!(objects[0].dependencies, vec!["fcstd:native:object#Sketch"]);
    let support = properties
        .iter()
        .find(|property| property.name == "Support")
        .expect("support");
    assert_eq!(support.owner, "fcstd:native:object#Body");
    assert_eq!(
        support.links[0].object.as_deref(),
        Some("fcstd:native:object#Sketch")
    );
    assert_eq!(support.family, crate::native::PropertyFamily::Link);
    assert_eq!(support.links[0].subelements, vec!["Face1"]);
    assert_eq!(
        support.dynamic.as_ref().and_then(|meta| meta.read_only),
        Some(true)
    );
    let members = properties
        .iter()
        .find(|property| property.name == "Members")
        .expect("members");
    assert_eq!(members.links.len(), 2);
    assert_eq!(
        members.links[0].object.as_deref(),
        Some("fcstd:native:object#Sketch")
    );
    assert_eq!(members.links[1].object.as_deref(), Some(""));
    let transient = properties
        .iter()
        .find(|property| property.name == "TransientState")
        .expect("transient");
    assert!(transient.transient);
    assert_eq!(transient.status, Some(8));
    let payload = properties
        .iter()
        .find(|property| property.name == "Payload")
        .expect("payload");
    assert_eq!(payload.side_entries, vec!["Payload.bin"]);
    let shape = properties
        .iter()
        .find(|property| property.name == "Shape")
        .expect("shape");
    assert_eq!(shape.family, crate::native::PropertyFamily::Geometry);
    assert_eq!(shape.side_entries, vec!["Shape.brp"]);
    let shape_payloads = namespace
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .expect("shape payloads");
    assert_eq!(shape_payloads.len(), 1);
    assert_eq!(
        shape_payloads[0]
            .text
            .as_ref()
            .map(|facts| facts.topology_version),
        Some(1)
    );
    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.curves.len(), 8);
    match &result.ir.model.curves[0].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction } => {
            assert_eq!([origin.x, origin.y, origin.z], [10.0, 20.0, 30.0]);
            assert_eq!([direction.x, direction.y, direction.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected curve {other:?}"),
    }
    match &result.ir.model.curves[1].geometry {
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(nurbs) => {
            assert_eq!(nurbs.degree, 2);
            assert_eq!(nurbs.control_points.len(), 3);
            assert_eq!(nurbs.knots, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            assert!(nurbs.weights.is_none());
        }
        other => panic!("unexpected curve {other:?}"),
    }
    assert_eq!(result.ir.model.procedural_curves.len(), 2);
    match &result.ir.model.procedural_curves[0].definition {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
            parameter_range, ..
        } => assert_eq!(*parameter_range, [0.0, 5.0]),
        other => panic!("unexpected trimmed construction {other:?}"),
    }
    match &result.ir.model.procedural_curves[1].definition {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset {
            distance,
            direction,
            ..
        } => {
            assert_eq!(*distance, 2.0);
            let direction = direction.expect("offset direction");
            assert_eq!([direction.x, direction.y, direction.z], [0.0, 0.0, 1.0]);
        }
        other => panic!("unexpected offset construction {other:?}"),
    }
    assert_eq!(result.ir.model.surfaces.len(), 7);
    match &result.ir.model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            assert_eq!([origin.x, origin.y, origin.z], [0.0, 0.0, 0.0]);
            assert_eq!([normal.x, normal.y, normal.z], [0.0, 0.0, 1.0]);
            assert_eq!([u_axis.x, u_axis.y, u_axis.z], [1.0, 0.0, 0.0]);
        }
        other => panic!("unexpected surface {other:?}"),
    }
    assert_eq!(result.ir.model.procedural_surfaces.len(), 4);
    assert!(matches!(
        result.ir.model.procedural_surfaces[0].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion { .. }
    ));
    assert!(matches!(
        result.ir.model.procedural_surfaces[1].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            parameter_interval: None,
            ..
        }
    ));
    assert!(matches!(
        result.ir.model.procedural_surfaces[2].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset {
            u_sense: None,
            v_sense: None,
            ..
        }
    ));
    assert!(matches!(
        result.ir.model.procedural_surfaces[3].definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Subset { .. }
    ));
    match &result.ir.model.surfaces[1].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) => {
            assert_eq!((nurbs.u_degree, nurbs.v_degree), (1, 1));
            assert_eq!((nurbs.u_count, nurbs.v_count), (2, 2));
            assert_eq!(nurbs.control_points.len(), 4);
            assert_eq!(nurbs.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
            assert_eq!(nurbs.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
            assert!(nurbs.weights.is_none());
        }
        other => panic!("unexpected surface {other:?}"),
    }
    assert_eq!(result.ir.model.tessellations.len(), 1);
    assert_eq!(result.ir.model.tessellations[0].vertices.len(), 3);
    assert_eq!(result.ir.model.tessellations[0].triangles, [[0, 1, 2]]);
    assert!(result.ir.model.tessellations[0].body.is_none());
    assert!(result.ir.model.tessellations[0].faces.is_empty());
    assert_eq!(
        result.ir.model.tessellations[0].chordal_deflection,
        Some(0.01)
    );
    let entries = namespace
        .arena_as::<crate::native::EntryRecord>("entries")
        .expect("entries");
    let payload_entry = entries
        .iter()
        .find(|entry| entry.name == "Payload.bin")
        .expect("payload entry");
    assert_eq!(payload_entry.referenced_by, vec![payload.id.clone()]);
    assert_eq!(payload_entry.data, b"payload");
    let ledger = namespace
        .arena_as::<crate::native::LogicalSpan>("logical_ledger")
        .expect("logical ledger");
    for entry in &entries {
        let mut spans = ledger
            .iter()
            .filter(|span| span.entry == entry.name)
            .collect::<Vec<_>>();
        spans.sort_by_key(|span| span.start);
        assert_eq!(spans.first().map(|span| span.start), Some(0));
        assert_eq!(spans.last().map(|span| span.end), Some(entry.byte_len));
        assert!(spans.windows(2).all(|pair| pair[0].end == pair[1].start));
    }
    assert!(ledger
        .iter()
        .filter(|span| span.entry == "Shape.brp")
        .all(|span| span.classification == "typed"));
    assert!(ledger
        .iter()
        .filter(|span| span.entry == "Payload.bin")
        .all(|span| span.classification == "named_opaque"));
    assert!(ledger
        .iter()
        .any(|span| span.entry == "Document.xml" && span.classification == "typed"));
    assert!(ledger
        .iter()
        .any(|span| span.entry == "Document.xml" && span.classification == "structural"));
    let coverage = namespace
        .arena_as::<crate::native::ByteCoverageRecord>("byte_coverage")
        .expect("byte coverage");
    assert_eq!(coverage.len(), 1);
    assert!(coverage[0].exact);
    assert_eq!(coverage[0].logical_entry_count, entries.len());
    assert_eq!(
        coverage[0].logical_byte_len,
        entries.iter().map(|entry| entry.byte_len).sum::<u64>()
    );
    assert_eq!(
        coverage[0].classification_bytes.values().sum::<u64>(),
        coverage[0].logical_byte_len
    );
    assert!(coverage[0]
        .named_opaque_entries
        .contains(&"Payload.bin".to_owned()));
    let findings = crate::validate_native(&result.ir);
    assert!(findings.is_empty(), "{findings:#?}");

    let mut corrupted = result.ir.clone();
    let missing_payload = ledger
        .iter()
        .filter(|span| span.entry != "Payload.bin")
        .cloned()
        .collect::<Vec<_>>();
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("logical_ledger", &missing_payload)
        .expect("replace logical ledger");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding
            .message
            .contains("logical ledger omits nonempty entry Payload.bin")));

    let mut corrupted = result.ir.clone();
    let mut invalid_owner = ledger.clone();
    invalid_owner
        .iter_mut()
        .find(|span| span.classification == "typed")
        .expect("typed span")
        .owner = None;
    corrupted
        .native
        .namespace_mut("fcstd")
        .set_arena("logical_ledger", &invalid_owner)
        .expect("replace logical ledger");
    assert!(crate::validate_native(&corrupted)
        .iter()
        .any(|finding| finding.message.contains("invalid logical entry or owner")));
}

#[test]
fn detects_marker_but_not_arbitrary_zip() {
    assert_eq!(
        FcstdCodec.detect(&archive(
            "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>"
        )),
        Confidence::High
    );
    let public = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/freecad_fcstd/fixtures/core_design_product.FCStd"
    ));
    assert_eq!(FcstdCodec.detect(&public[..512]), Confidence::High);
    assert_eq!(FcstdCodec.detect(b"PK\x03\x04 unrelated"), Confidence::Low);
    assert_eq!(FcstdCodec.detect(b"not zip"), Confidence::No);
}

#[test]
fn inspects_and_closes_physical_ledger() {
    let bytes = archive("<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\"><Object/></Document>");
    let archive_len = bytes.len() as u64;
    let summary = FcstdCodec
        .inspect(&mut Cursor::new(&bytes))
        .expect("inspect");
    assert_eq!(summary.format, "fcstd");
    assert!(summary.notes.iter().any(|note| note == "SchemaVersion=4"));
    let result = FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                container_only: true,
            },
        )
        .expect("decode");
    assert!(result.report.losses.is_empty());
    let ledger = result
        .ir
        .native
        .namespace("fcstd")
        .expect("namespace")
        .arena_as::<crate::native::ArchiveSpan>("physical_ledger")
        .expect("ledger");
    assert_eq!(ledger.first().map(|span| span.start), Some(0));
    assert_eq!(ledger.last().map(|span| span.end), Some(archive_len));
    assert!(ledger.windows(2).all(|pair| pair[0].end == pair[1].start));
    assert!(crate::validate_native(&result.ir).is_empty());
    for role in [
        "local-signature",
        "local-fields",
        "local-name",
        "compressed-payload",
        "central-signature",
        "central-fields",
        "central-name",
        "end-record",
    ] {
        assert!(
            ledger.iter().any(|span| span.role == role),
            "missing {role}"
        );
    }
}
