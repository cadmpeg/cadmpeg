// SPDX-License-Identifier: Apache-2.0
//! Design booleans-patterns transfer unit tests.

use crate::test_support::*;
use crate::FcstdCodec;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

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
            .ir()
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
    let cadmpeg_ir::features::FeatureDefinition::Combine {
        target,
        tools,
        op,
        keep_tools,
    } = &feature("Fuse").definition
    else {
        panic!("multi-fuse");
    };
    assert_eq!(*op, cadmpeg_ir::features::BooleanOp::Join);
    assert!(!keep_tools);
    assert!(matches!(
        target,
        cadmpeg_ir::features::BodySelection::Native(value) if value.ends_with(":link:0")
    ));
    assert!(matches!(
        tools,
        cadmpeg_ir::features::BodySelection::Native(value) if value.ends_with(":links:1..2")
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
pub(crate) fn transfers_partdesign_boolean_base_and_group_rules() {
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
            .ir()
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
            keep_tools: false,
        } if target.ends_with(":Group:link:2")
            && tools.ends_with(":Group:links:0..2")
    ));
    assert!(matches!(
        definition("Cut"),
        cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(target),
            tools: cadmpeg_ir::features::BodySelection::Native(tools),
            op: cadmpeg_ir::features::BooleanOp::Cut,
            keep_tools: false,
        } if target.ends_with(":BaseFeature") && tools.ends_with(":Group")
    ));
    assert!(result.report().losses.is_empty());
}

#[test]
pub(crate) fn transfers_uniform_irregular_and_two_axis_patterns() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6">
 <Object type="PartDesign::LinearPattern" name="Uniform" id="2"/>
 <Object type="PartDesign::LinearPattern" name="Custom" id="3"/>
 <Object type="PartDesign::LinearPattern" name="TwoAxis" id="4"/>
 <Object type="PartDesign::PolarPattern" name="PolarCustom" id="5"/>
 <Object type="PartDesign::LinearPattern" name="NativeDirection" id="6"/>
 <Object type="PartDesign::Feature" name="Seed" id="1"/>
</Objects>
<ObjectData Count="6">
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
 <Object name="NativeDirection"><Properties Count="4">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Direction" type="App::PropertyLinkSub"><LinkSub value="Seed" count="1"><Sub value="Edge1"/></LinkSub></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
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
            .ir()
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
    assert!(matches!(
        &feature("NativeDirection").definition,
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::Linear {
                direction: None,
                spacing: cadmpeg_ir::features::Length(4.0),
                count: 3,
                ..
            },
            ..
        }
    ));
    assert_eq!(feature("Uniform").dependencies.len(), 1);
    assert!(result.report().losses.is_empty());
    assert_valid_document(result.ir());
    let census = result
        .ir()
        .native
        .namespace("fcstd")
        .expect("native namespace")
        .arena_as::<crate::native::DesignCensusRecord>("design_census")
        .expect("design census");
    assert_eq!(census.len(), 6);
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
    let baseline_findings = cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).findings;
    assert!(
        baseline_findings
            .iter()
            .all(|finding| finding.check != cadmpeg_ir::Check::Identity),
        "{baseline_findings:?}"
    );
    let mut corrupted = result.ir().clone();
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
fn resolves_datum_references_for_polar_and_mirror_patterns() {
    let document = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="7">
 <Object type="Part::Box" name="Seed" id="1"/>
 <Object type="App::Line" name="Axis" id="2"/>
 <Object type="App::Plane" name="Plane" id="3"/>
 <Object type="PartDesign::PolarPattern" name="Ring" id="4"/>
 <Object type="PartDesign::Mirrored" name="Mirror" id="5"/>
 <Object type="PartDesign::Body" name="Body" id="6"/>
 <Object type="PartDesign::Mirrored" name="FaceMirror" id="7"/>
</Objects>
<ObjectData Count="7">
 <Object name="Seed"><Properties Count="3"><Property name="Length" type="App::PropertyLength"><Float value="1"/></Property><Property name="Width" type="App::PropertyLength"><Float value="1"/></Property><Property name="Height" type="App::PropertyLength"><Float value="1"/></Property></Properties></Object>
 <Object name="Axis"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="2" Pz="3" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Plane"><Properties Count="1"><Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="4" Py="5" Pz="6" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
 <Object name="Ring"><Properties Count="5">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="1"><Link value="Seed"/></LinkList></Property>
  <Property name="Axis" type="App::PropertyLinkSub"><LinkSub value="Axis" count="1"><Sub value=""/></LinkSub></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="360"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="4"/></Property>
 </Properties></Object>
 <Object name="Mirror"><Properties Count="2">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="0"/></Property>
  <Property name="MirrorPlane" type="App::PropertyLinkSub"><LinkSub value="Plane" count="1"><Sub value=""/></LinkSub></Property>
 </Properties></Object>
 <Object name="FaceMirror"><Properties Count="2">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="2"><Link value="Seed"/><Link value="Plane"/></LinkList></Property>
  <Property name="MirrorPlane" type="App::PropertyLinkSub"><LinkSub value="Seed" count="1"><Sub value="Face1"/></LinkSub></Property>
 </Properties></Object>
 <Object name="Body"><Properties Count="1">
  <Property name="Group" type="App::PropertyLinkList"><LinkList count="3"><Link value="Seed"/><Link value="Mirror"/><Link value="FaceMirror"/></LinkList></Property>
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
            .ir()
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
    assert!(matches!(
        definition("FaceMirror"),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            pattern: cadmpeg_ir::features::PatternKind::MirrorReference {
                plane: cadmpeg_ir::features::FaceSelection::Native(plane),
            },
            ..
        } if plane.ends_with(":MirrorPlane")
    ));
    assert!(result.report().losses.is_empty());
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
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="0"/></Property>
  <Property name="Direction" type="App::PropertyVector"><Vector x="1" y="0" z="0"/></Property>
  <Property name="Mode" type="App::PropertyEnumeration"><Integer value="0"/></Property>
  <Property name="Length" type="App::PropertyLength"><Float value="8"/></Property>
  <Property name="Occurrences" type="App::PropertyInteger"><Integer value="3"/></Property>
 </Properties></Object>
 <Object name="Scaled"><Properties Count="3">
  <Property name="Originals" type="App::PropertyLinkList"><LinkList count="0"/></Property>
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
            .ir()
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
    assert!(result.report().losses.is_empty());
}
