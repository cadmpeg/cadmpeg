use std::io::{Cursor, Write};

use cadmpeg_ir::{Codec, Confidence, DecodeOptions};
use zip::write::SimpleFileOptions;

use crate::FcstdCodec;

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
 <Object name="Fillet"><Properties Count="2">
  <Property name="Base" type="App::PropertyLinkSub"><Link object="Revolution" sub="Edge1"/></Property>
  <Property name="Radius" type="App::PropertyLength"><Float value="2"/></Property>
 </Properties></Object>
 <Object name="Chamfer"><Properties Count="4">
  <Property name="Base" type="App::PropertyLinkSub"><Link object="Fillet" sub="Edge2"/></Property>
  <Property name="ChamferType" type="App::PropertyEnumeration"><Integer value="2"/></Property>
  <Property name="Size" type="App::PropertyLength"><Float value="1.5"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="30"/></Property>
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
            ..
        } if (angle.0 - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    ));
}

#[test]
fn reports_attributable_native_design_blockers() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="PartDesign::FeatureCustom" name="Custom" id="1"/></Objects>
<ObjectData Count="1"><Object name="Custom"><Properties Count="0"/></Object></ObjectData>
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
        Some("fcstd:object:Custom")
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
 <Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="2" xlink="1">
  <Cell address="A1" content="5" alias="width" displayUnit="mm"/>
  <Cell address="A2" content="=width * 3" alias="height" style="bold"/>
 </Cells></Property></Properties></Object>
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
}

#[test]
fn recovers_product_prototypes_occurrences_and_placements() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="3">
 <Object type="App::Part" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="Prototype" id="2"/>
 <Object type="App::Link" name="Occurrence" id="3"/>
</Objects>
<ObjectData Count="3">
 <Object name="Assembly"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Occurrence"/></LinkList></Property></Properties></Object>
 <Object name="Prototype"><Properties Count="0"/></Object>
 <Object name="Occurrence"><Properties Count="6">
  <Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="Prototype"/></Property>
  <Property name="LinkPlacement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="ElementCount" type="App::PropertyIntegerConstraint"><Integer value="2"/></Property>
  <Property name="LinkTransform" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="PlacementList" type="App::PropertyPlacementList"><PlacementList file="PlacementList"/></Property>
  <Property name="ScaleList" type="App::PropertyVectorList"><VectorList file="ScaleList"/></Property>
 </Properties></Object>
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
    assert_eq!(nodes.len(), 2);
    let assembly = nodes.iter().find(|node| node.kind == "part").expect("part");
    let occurrence = nodes
        .iter()
        .find(|node| node.kind == "occurrence")
        .expect("occurrence");
    assert_eq!(assembly.members, vec![occurrence.object.clone()]);
    assert_eq!(
        occurrence.prototype.as_deref(),
        Some("fcstd:object:Prototype")
    );
    assert_eq!(occurrence.local_transform.expect("placement")[0][3], 4.0);
    assert_eq!(occurrence.element_count, Some(2));
    assert_eq!(occurrence.link_transform, Some(true));
    assert_eq!(occurrence.element_transforms.len(), 2);
    assert_eq!(occurrence.element_transforms[1][0][3], 4.0);
    assert_eq!(occurrence.element_scales, vec![[1.0; 3], [2.0; 3]]);
    assert!(crate::validate_native(&result.ir).is_empty());
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
 <Object name="Joint"><Properties Count="6">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1"/><CustomEnumList count="2"><Enum value="Fixed"/><Enum value="Revolute"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="2"><Sub value="A.Face1"/><Sub value="A.Edge2"/></XLink></Property>
  <Property name="Reference2" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="1"><Sub value="B.Edge3"/></XLink></Property>
  <Property name="Placement1" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement2" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/></Property>
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
        Some("fcstd:object:Assembly")
    );
    assert_eq!(joints[0].references[0].subelements, ["A.Face1", "A.Edge2"]);
    assert_eq!(joints[0].placements[1][0][3], 2.0);
    assert_eq!(
        joints[0].parameters.get("Suppressed").map(String::as_str),
        Some("true")
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
        .find(|drawing| drawing.object.ends_with(":Page"))
        .expect("page");
    let template = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with(":Template"))
        .expect("template");
    let view = drawings
        .iter()
        .find(|drawing| drawing.object.ends_with(":View"))
        .expect("view");
    assert_eq!(page.template.as_deref(), Some("fcstd:object:Template"));
    assert_eq!(page.views, ["fcstd:object:View"]);
    assert_eq!(template.side_entries, ["page.svg"]);
    assert_eq!(
        view.sources[0].object.as_deref(),
        Some("fcstd:object:Model")
    );
    assert!(view.parameters.contains_key("Direction"));
    assert!(crate::validate_native(&result.ir).is_empty());
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
    assert_eq!(result.ir.model.sketches[0].origin.x, 1.0);
    assert!((result.ir.model.sketches[0].normal.y + 1.0).abs() < 1e-12);
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
        Some("fcstd:object:Pocket")
    );
    assert!(pocket.suppressed);
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
    brep.push(0);
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
<ViewProvider name="Shape" expanded="1"><Properties Count="3">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3368601600"/></Property>
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
    assert_eq!(result.ir.model.appearances.len(), 1);
    assert_eq!(result.ir.model.appearance_bindings.len(), 1);
    assert_eq!(result.ir.model.bodies[0].visible, Some(false));
    let color = result.ir.model.bodies[0].color.expect("shape color");
    assert!((color.r - 200.0 / 255.0).abs() < 1e-6);
    assert!((color.a - 0.75).abs() < 1e-6);
    let namespace = result.ir.native.namespace("fcstd").expect("native");
    assert_eq!(namespace.version, 7);
    let gui_providers = namespace
        .arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")
        .expect("GUI providers");
    let gui_properties = namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")
        .expect("GUI properties");
    assert_eq!(gui_providers.len(), 1);
    assert_eq!(
        gui_providers[0].object.as_deref(),
        Some("fcstd:object:Shape")
    );
    assert_eq!(gui_properties.len(), 3);
    assert!(gui_properties
        .iter()
        .all(|property| property.raw_xml.starts_with("<Property")));
    assert!(crate::validate_native(&result.ir).is_empty());
    assert!(result
        .ir
        .model
        .coedges
        .iter()
        .all(|coedge| coedge.pcurve.is_some()));
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
    let gui = br#"<Document SchemaVersion="1"><ViewProviderData Count="1"><ViewProvider name="Shape"><Properties Count="2">
<Property name="ShapeColor" type="App::PropertyColor"><PropertyColor value="3435973632"/></Property>
<Property name="DiffuseColor" type="App::PropertyColorList"><ColorList file="DiffuseColor"/></Property>
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
    let bytes = archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("GuiDocument.xml", gui),
        ("DiffuseColor", &face_colors),
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
    assert!(crate::validate_native(&result.ir).is_empty());
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
    assert_ne!(first.pcurve, second.pcurve);
    assert!(first.pcurve.is_some() && second.pcurve.is_some());
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
    let unknowns = result.ir.native_unknowns("fcstd").expect("unknowns");
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
    assert_eq!(extensions[0].owner, "fcstd:object:Body");
    let extension_value = properties
        .iter()
        .find(|property| property.name == "ExtensionValue")
        .expect("extension property");
    assert_eq!(extension_value.owner, extensions[0].id);
    assert_eq!(objects[0].dependencies, vec!["fcstd:object:Sketch"]);
    let support = properties
        .iter()
        .find(|property| property.name == "Support")
        .expect("support");
    assert_eq!(support.owner, "fcstd:object:Body");
    assert_eq!(
        support.links[0].object.as_deref(),
        Some("fcstd:object:Sketch")
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
        Some("fcstd:object:Sketch")
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
    let findings = crate::validate_native(&result.ir);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn detects_marker_but_not_arbitrary_zip() {
    assert_eq!(
        FcstdCodec.detect(&archive(
            "<Document SchemaVersion=\"4\" FileVersion=\"1\"/>"
        )),
        Confidence::High
    );
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
