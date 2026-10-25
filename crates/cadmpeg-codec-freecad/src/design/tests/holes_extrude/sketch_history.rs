// SPDX-License-Identifier: Apache-2.0
//! Sketch, pad and pocket history transferred as one authored dependency graph.

use crate::test_support::test_archive::archive;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

#[test]
fn transfers_sketch_pad_and_pocket_design_history() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="4" Dependencies="1">
  <ObjectDeps Name="Body" Count="0"/>
  <ObjectDeps Name="Sketch" Count="0"/>
  <ObjectDeps Name="Pad" Count="1"><Dep Name="Sketch"/></ObjectDeps>
  <ObjectDeps Name="Pocket" Count="2"><Dep Name="Pad"/><Dep Name="Sketch"/></ObjectDeps>
  <Object type="PartDesign::Body" name="Body" id="1"/>
  <Object type="Sketcher::SketchObject" name="Sketch" id="1"/>
  <Object type="PartDesign::Pad" name="Pad" id="2"/>
  <Object type="PartDesign::Pocket" name="Pocket" id="3"/>
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
    <Property name="Sketch" type="App::PropertyLink"><Link value="Sketch"/></Property>
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
    assert_eq!(result.ir().model.sketches.len(), 1);
    assert_eq!(result.ir().model.sketch_entities.len(), 4);
    let profiles = &result.ir().model.sketches[0].profiles;
    assert_eq!(profiles.len(), 4);
    assert!(profiles.iter().all(|profile| profile.len() == 1));
    let (origin, normal, _) = result.ir().model.sketches[0]
        .resolved_placement()
        .expect("resolved sketch placement");
    assert_eq!(origin.x, 1.0);
    assert!((normal.y + 1.0).abs() < 1.0e-12);
    assert_eq!(result.ir().model.features.len(), 4);
    assert_eq!(result.ir().model.sketch_constraints.len(), 2);
    assert_eq!(result.ir().model.parameters.len(), 3);
    assert!(result
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition.kind(),
                cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Horizontal { .. }
            )
        }));
    assert!(result
        .ir()
        .model
        .sketch_constraints
        .iter()
        .any(|constraint| {
            matches!(
                constraint.definition.kind(),
                cadmpeg_ir::sketches::SketchConstraintDefinitionInput::HorizontalDistance { .. }
            )
        }));
    let pad = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pad"))
        .expect("pad");
    let pocket = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Pocket"))
        .expect("pocket");
    let body = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Body"))
        .expect("body");
    assert_eq!(result.ir().model.feature_parent(&pad.id), Some(&body.id));
    assert_eq!(result.ir().model.feature_parent(&pocket.id), Some(&body.id));
    assert!(body.source_properties.is_empty());
    assert!(
        matches!(body.definition().operation(), cadmpeg_ir::features::FeatureOperation::TreeNode { children, .. } if children.active_child().as_ref() == Some(&pocket.id))
    );
    assert_eq!(pocket.suppressed, Some(true));
    assert!(!pocket.source_properties.contains_key("Suppressed"));
    let pocket_length = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| {
            parameter.owner.as_ref() == Some(&pocket.id) && parameter.name == "Length"
        })
        .expect("pocket length");
    let pad_length = result
        .ir()
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.owner.as_ref() == Some(&pad.id) && parameter.name == "Length")
        .unwrap();
    assert_eq!(
        pocket_length.expression.tokens,
        vec![
            cadmpeg_ir::features::ExpressionToken::Parameter(pad_length.id.clone()),
            cadmpeg_ir::features::ExpressionToken::Text(" / 4".into())
        ]
    );
    assert!(matches!(
        pad.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::Sketch(_)),
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Join,
            ..
        }) if result.ir().dimension_value(actual_length).expect("resolved dimension").get() == 10.0
    ));
    assert!(matches!(
        pocket.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Extrude {
            extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                side: cadmpeg_ir::features::ExtrudeSide {
                    termination: cadmpeg_ir::features::LinearTermination::Blind {
                        length: actual_length
                    },
                    ..
                }
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }) if result.ir().dimension_value(actual_length).expect("resolved dimension").get() == 2.5
    ));
    let native_findings = crate::test_support::validate_native(result.ir());
    assert!(native_findings.is_empty(), "{native_findings:#?}");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
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
