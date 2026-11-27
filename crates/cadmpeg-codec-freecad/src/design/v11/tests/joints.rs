use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::container::{Container, UnorderedKind};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::parameter::ParameterOwner;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use super::{cadir_round_trip, decode, group_members};
use crate::test_support::test_archive::{archive, assert_valid_document};
use crate::FcstdCodec;

const ASSEMBLY: &str = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="9">
 <Object type="Assembly::AssemblyObject" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="BracketShape" id="2"/>
 <Object type="Part::Feature" name="PinShape" id="3"/>
 <Object type="App::Link" name="Bracket" id="4"/>
 <Object type="App::Link" name="Pin" id="5"/>
 <Object type="Assembly::JointGroup" name="Joints" id="6"/>
 <Object type="App::FeaturePython" name="Hinge" id="7"/>
 <Object type="App::FeaturePython" name="Ground" id="8"/>
 <Object type="App::FeaturePython" name="Weld" id="9"/>
</Objects>
<ObjectData Count="9">
 <Object name="Assembly"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="4"><Link value="Bracket"/><Link value="Pin"/><Link value="Joints"/><Link value="Weld"/></LinkList></Property></Properties></Object>
 <Object name="BracketShape"><Properties Count="0"/></Object>
 <Object name="PinShape"><Properties Count="0"/></Object>
 <Object name="Bracket"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="BracketShape"/></Property></Properties></Object>
 <Object name="Pin"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="PinShape"/></Property></Properties></Object>
 <Object name="Joints"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="2"><Link value="Hinge"/><Link value="Ground"/></LinkList></Property></Properties></Object>
 <Object name="Hinge"><Properties Count="14">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="3"><Enum value="Fixed"/><Enum value="Revolute"/><Enum value="Weld"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="2"><Sub value="Bracket.Face1"/><Sub value="Bracket.Vertex2"/></XLink></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Edge3"/></Property>
  <Property name="Placement1" type="App::PropertyPlacement"><PropertyPlacement Px="1" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Placement2" type="App::PropertyPlacement"><PropertyPlacement Px="2" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Offset1" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0.5" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Suppressed" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="15"/></Property>
  <Property name="AngleMin" type="App::PropertyAngle"><Float value="-30"/></Property>
  <Property name="AngleMax" type="App::PropertyAngle"><Float value="45"/></Property>
  <Property name="EnableAngleMin" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="EnableAngleMax" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="LengthMin" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="Detach1" type="App::PropertyBool"><Bool value="false"/></Property>
 </Properties></Object>
 <Object name="Ground"><Properties Count="2">
  <Property name="ObjectToGround" type="App::PropertyLinkGlobal"><Link value="Bracket"/></Property>
  <Property name="Placement" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
 </Properties></Object>
 <Object name="Weld"><Properties Count="3">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="2" CustomEnum="true"/><CustomEnumList count="3"><Enum value="Fixed"/><Enum value="Revolute"/><Enum value="Weld"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;

fn joint_feature<'ir>(
    ir: &'ir CadIr,
    name: &str,
) -> (
    &'ir cadmpeg_ir::design::feature::Feature,
    &'ir cadmpeg_ir::design::ops::shared::Joint,
) {
    let feature = ir
        .design
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("{:?}", ir.design.features));
    let FeatureOperation::Joint(joint) = &feature.definition.operation else {
        panic!("{:?}", feature.definition.operation);
    };
    (feature, joint)
}

/// The option `fcstd:<name>` of `options`.
fn option(
    options: &cadmpeg_ir::design::options::NativeOptions,
    name: &str,
) -> Option<cadmpeg_ir::design::options::NativeOption> {
    options
        .iter()
        .find(|(key, _)| key.as_str() == format!("fcstd:{name}"))
        .map(|(_, option)| option.clone())
}

/// The construction of frame connector `connector`.
fn frame(
    connector: &cadmpeg_ir::design::ops::shared::JointConnector,
) -> &cadmpeg_ir::design::construction::Construction<cadmpeg_ir::design::construction::kinds::Frame>
{
    let cadmpeg_ir::design::ops::shared::JointConnector::Frame(frame) = connector else {
        panic!("{connector:?}");
    };
    frame
}

/// The native names and occurrence paths of the picks of frame connector
/// `connector`.
fn frame_picks(
    connector: &cadmpeg_ir::design::ops::shared::JointConnector,
) -> Vec<(Option<String>, Vec<String>)> {
    use cadmpeg_ir::design::construction::InputReference;
    use cadmpeg_ir::design::selection::{PickContext, Picks, StepRef};
    let frame = frame(connector);
    let [input] = frame.inputs.as_slice() else {
        panic!("{:?}", frame.inputs);
    };
    let Some(InputReference::Picks(selection)) = &input.reference else {
        panic!("{input:?}");
    };
    let Picks::Named(members) = &selection.picks else {
        panic!("{selection:?}");
    };
    let picks = members
        .as_slice()
        .iter()
        .map(|pick| {
            let native = pick.native.as_ref().map(|name| match &name.value {
                cadmpeg_ir::design::names::NativeNameValue::Text(text) => text.as_str().to_owned(),
                other => panic!("{other:?}"),
            });
            let path = match &pick.context {
                Some(PickContext::Occurrence(path)) => path
                    .0
                    .iter()
                    .map(|step| match &step.occurrence {
                        StepRef::Occurrence(id) => id.as_str().to_owned(),
                        StepRef::Native(name) => panic!("{name:?}"),
                    })
                    .collect(),
                None => Vec::new(),
                Some(other) => panic!("{other:?}"),
            };
            (native, path)
        })
        .collect();
    picks
}

#[test]
fn an_assembly_joint_is_a_joint_feature_of_its_joint_group() {
    use cadmpeg_ir::design::construction::{FrameAxis, PostStep};
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::ops::shared::{AxisLimits, JointKind, JointMotion};
    use cadmpeg_ir::design::options::NativeOption;
    use cadmpeg_ir::design::selection::PickTarget;

    let ir = decode(&archive(ASSEMBLY), DecodeDepth::Design);
    let occurrence = |object: &str| {
        let native = format!("fcstd:native:object#{object}");
        ir.model
            .occurrences
            .iter()
            .find(|occurrence| {
                occurrence.native_ref.as_deref() == Some(native.as_str())
                    && !occurrence.id.as_str().ends_with(":container")
            })
            .unwrap_or_else(|| panic!("{:?}", ir.model.occurrences))
            .id
            .clone()
    };
    let (bracket, pin) = (occurrence("Bracket"), occurrence("Pin"));
    let assembly = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.native_ref.as_deref() == Some("fcstd:native:object#Assembly"))
        .unwrap()
        .id
        .clone();

    let (feature, hinge) = joint_feature(&ir, "Hinge");
    assert_eq!(feature.id.as_str(), "fcstd:model:joint#Hinge:constraint");
    assert_eq!(feature.suppressed, Some(Dimension::Literal(true)));
    assert_eq!(feature.component.as_ref(), Some(&assembly));
    assert_eq!(
        feature.native_ref.as_deref(),
        Some("fcstd:native:object#Hinge")
    );
    // FreeCAD turns and slides joints about the connector frame's z axis;
    // an enabled limit is in degrees (§6.7).
    let JointKind::Motion {
        motion: JointMotion::Revolute { axis: FrameAxis::Z },
        limits,
        ..
    } = &hinge.kind
    else {
        panic!("{:?}", hinge.kind);
    };
    let [AxisLimits::Rotation {
        axis: FrameAxis::Z,
        limits: Some(bounds),
        rest: None,
    }] = limits.as_slice()
    else {
        panic!("{limits:?}");
    };
    let degrees = |bound: &Option<Dimension<cadmpeg_ir::scalar::Angle>>| match bound {
        Some(Dimension::Literal(angle)) => angle.get().to_degrees(),
        other => panic!("{other:?}"),
    };
    assert!((degrees(&bounds.min) + 30.0).abs() < 1e-9);
    assert!((degrees(&bounds.max) - 45.0).abs() < 1e-9);

    // A connector picks its elements in the occurrence the sub-name names;
    // its offset is its step and its placement its resolved frame.
    let (first, picks) = (
        frame(&hinge.connectors[0]),
        frame_picks(&hinge.connectors[0]),
    );
    let bracket_path = vec![bracket.as_str().to_owned()];
    assert_eq!(
        picks,
        [
            (Some("Face1".to_owned()), bracket_path.clone()),
            (Some("Vertex2".to_owned()), bracket_path.clone())
        ]
    );
    let [PostStep::Fixed(offset)] = first.after.as_slice() else {
        panic!("{:?}", first.after);
    };
    assert_eq!(offset.affine_rows()[2][3], 0.5);
    let origin = first.resolved.as_ref().unwrap().origin.get();
    assert_eq!([origin.x, origin.y, origin.z], [1.0, 0.0, 0.0]);
    let (second, picks) = (
        frame(&hinge.connectors[1]),
        frame_picks(&hinge.connectors[1]),
    );
    assert_eq!(
        picks,
        [(Some("Edge3".to_owned()), vec![pin.as_str().to_owned()])]
    );
    assert!(second.after.is_empty());
    assert_eq!(second.resolved.as_ref().unwrap().origin.get().x, 2.0);

    // A value the joint type does not use is a source setting (Rule N case
    // (b)).
    assert!(
        hinge.native_options.is_empty(),
        "{:?}",
        hinge.native_options
    );
    let Some(NativeOption::Angle(Dimension::Literal(angle))) =
        option(&hinge.source_settings, "Angle")
    else {
        panic!("{:?}", hinge.source_settings);
    };
    assert!((angle.get().to_degrees() - 15.0).abs() < 1e-9);
    let Some(NativeOption::Length(Dimension::Literal(length))) =
        option(&hinge.source_settings, "LengthMin")
    else {
        panic!("{:?}", hinge.source_settings);
    };
    assert_eq!(length.get(), 1.0);
    for used in ["AngleMin", "AngleMax", "EnableAngleMin", "Suppressed"] {
        assert_eq!(option(&hinge.source_settings, used), None, "{used}");
    }
    assert_eq!(
        option(&hinge.source_settings, "Detach1"),
        Some(NativeOption::Bool(Dimension::Literal(false)))
    );

    // An unknown family is a native kind; a reference with no element picks
    // the whole occurrence.
    let (weld_feature, weld) = joint_feature(&ir, "Weld");
    assert_eq!(
        weld.kind,
        JointKind::Native(cadmpeg_core::text::NonBlankString::new("Weld".to_owned()).unwrap())
    );
    let picks = frame_picks(&weld.connectors[0]);
    assert_eq!(picks, [(None, bracket_path)]);
    let cadmpeg_ir::design::construction::InputReference::Picks(selection) =
        frame(&weld.connectors[0]).inputs[0]
            .reference
            .as_ref()
            .unwrap()
    else {
        unreachable!();
    };
    let cadmpeg_ir::design::selection::Picks::Named(members) = &selection.picks else {
        unreachable!();
    };
    assert!(members
        .as_slice()
        .iter()
        .all(|pick| pick.target == PickTarget::Component));

    // A JointGroup is an unordered Group of its joints; a joint it does not
    // list is a member of the document's Group.
    let joints = ir
        .design
        .containers
        .iter()
        .find(|container| container.id().as_str() == "fcstd:design:container#Joints")
        .unwrap_or_else(|| panic!("{:?}", ir.design.containers));
    let Container::Unordered {
        kind: UnorderedKind::Group,
        members,
        ..
    } = joints
    else {
        panic!("{joints:?}");
    };
    assert_eq!(
        members,
        &BTreeSet::from([Member::Feature(feature.id.clone())])
    );
    assert!(ir.design.scopes[0].roots.contains(joints.id()));
    assert!(group_members(&ir).contains(&Member::Feature(weld_feature.id.clone())));

    // Grounding is occurrence state (§6.7).
    let scope = &ir.design.scopes[0];
    assert_eq!(
        scope
            .occurrences
            .get(&bracket)
            .and_then(|state| state.grounded),
        Some(true)
    );
    assert_eq!(scope.occurrences.len(), 1);

    let (written, omitted) = cadir_round_trip(&ir);
    for id in [feature.id.as_str(), weld_feature.id.as_str()] {
        assert!(!omitted.contains(id), "{omitted:?}");
    }
    assert_eq!(written.design.features, ir.design.features);
    assert_eq!(written.design.containers, ir.design.containers);
    assert_eq!(written.design.scopes, ir.design.scopes);
}

const JOINT_VALUES: &str = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="12">
 <Object type="Assembly::AssemblyObject" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="BracketShape" id="2"/>
 <Object type="Part::Feature" name="PinShape" id="3"/>
 <Object type="App::Link" name="Bracket" id="4"/>
 <Object type="App::Link" name="Pin" id="5"/>
 <Object type="App::FeaturePython" name="Turn" id="6"/>
 <Object type="App::FeaturePython" name="OldTurn" id="7"/>
 <Object type="App::FeaturePython" name="Gap" id="8"/>
 <Object type="App::FeaturePython" name="Slide" id="9"/>
 <Object type="App::FeaturePython" name="Held" id="10"/>
 <Object type="App::FeaturePython" name="Loose" id="11"/>
 <Object type="App::FeaturePython" name="Still" id="12"/>
</Objects>
<ObjectData Count="12">
 <Object name="Assembly"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="9"><Link value="Bracket"/><Link value="Pin"/><Link value="Turn"/><Link value="OldTurn"/><Link value="Gap"/><Link value="Slide"/><Link value="Held"/><Link value="Loose"/><Link value="Still"/></LinkList></Property></Properties></Object>
 <Object name="BracketShape"><Properties Count="0"/></Object>
 <Object name="PinShape"><Properties Count="0"/></Object>
 <Object name="Bracket"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="BracketShape"/></Property></Properties></Object>
 <Object name="Pin"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="PinShape"/></Property></Properties></Object>
 <Object name="Turn"><Properties Count="5">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="-330"/></Property>
  <Property name="Distance" type="App::PropertyLength"><Float value="5"/></Property>
 </Properties></Object>
 <Object name="OldTurn"><Properties Count="4">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Distance" type="App::PropertyFloat"><Float value="40"/></Property>
 </Properties></Object>
 <Object name="Gap"><Properties Count="4">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="2" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Distance" type="App::PropertyLength"><Float value="-2"/></Property>
 </Properties></Object>
 <Object name="Slide"><Properties Count="9">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="3" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="LengthMin" type="App::PropertyLength"><Float value="1"/></Property>
  <Property name="EnableLengthMin" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="LengthMax" type="App::PropertyLength"><Float value="9"/></Property>
  <Property name="EnableLengthMax" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="AngleMin" type="App::PropertyAngle"><Float value="5"/></Property>
  <Property name="EnableAngleMin" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Held"><Properties Count="6">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSubHidden"><XLink file="" name="Assembly" count="2"><Sub value="Bracket.Face1"/><Sub value="Bracket.Edge2"/></XLink></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Edge3"/></Property>
  <Property name="Placement1" type="App::PropertyPlacement"><PropertyPlacement Px="3" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Offset1" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="0.5" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Detach1" type="App::PropertyBool"><Bool value="true"/></Property>
 </Properties></Object>
 <Object name="Loose"><Properties Count="2">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket"/></Property>
 </Properties></Object>
 <Object name="Still"><Properties Count="4">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="-360"/></Property>
 </Properties></Object>
</ObjectData></Document>"#;

#[test]
fn joint_values_follow_what_the_joint_type_uses() {
    use cadmpeg_ir::design::construction::{FrameAxis, PostStep};
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::ops::shared::{Alignment, AxisLimits, JointKind, JointMotion};
    use cadmpeg_ir::design::options::NativeOption;

    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(JOINT_VALUES)),
            &DecodeOptions {
                depth: DecodeDepth::Design,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let ir = decoded.ir();
    assert_valid_document(ir);
    let degrees = |angle: &Option<Dimension<cadmpeg_ir::scalar::Angle>>| match angle {
        Some(Dimension::Literal(angle)) => angle.get().to_degrees(),
        other => panic!("{other:?}"),
    };

    // FreeCAD solves an Angle joint for the cosine of its angle: the angle
    // reflected into a half turn. The stored value and the unused distance
    // are source settings, and the joint states no alignment.
    let (_, turn) = joint_feature(ir, "Turn");
    let JointKind::Angle { angle, .. } = &turn.kind else {
        panic!("{:?}", turn.kind);
    };
    assert!((degrees(angle) - 30.0).abs() < 1e-9);
    assert_eq!(turn.alignment, None);
    let Some(NativeOption::Angle(Dimension::Literal(signed))) =
        option(&turn.source_settings, "Angle")
    else {
        panic!("{:?}", turn.source_settings);
    };
    assert!((signed.get().to_degrees() + 330.0).abs() < 1e-9);
    assert!(matches!(
        option(&turn.source_settings, "Distance"),
        Some(NativeOption::Length(Dimension::Literal(length))) if length.get() == 5.0
    ));
    assert!(turn.native_options.is_empty(), "{:?}", turn.native_options);

    // A float Distance is the old form, which stores an Angle joint's angle
    // in degrees.
    let (_, old) = joint_feature(ir, "OldTurn");
    let JointKind::Angle { angle, .. } = &old.kind else {
        panic!("{:?}", old.kind);
    };
    assert!((degrees(angle) - 40.0).abs() < 1e-9);
    assert!(old.source_settings.is_empty(), "{:?}", old.source_settings);

    // FreeCAD solves a Distance joint between faces of no shape in its
    // default case: the planes of the two frames, offset along the first z
    // axis by the signed distance.
    let (_, gap) = joint_feature(ir, "Gap");
    let JointKind::Motion {
        motion: JointMotion::Planar { axis: FrameAxis::Z },
        offset,
        ..
    } = &gap.kind
    else {
        panic!("{:?}", gap.kind);
    };
    assert!(matches!(offset.z, Some(Dimension::Literal(z)) if z.get() == -2.0));
    assert_eq!(option(&gap.native_options, "Distance"), None);
    assert_eq!(gap.alignment, Some(Alignment::Nearest));

    // An Angle joint at a full turn holds the axes parallel in the nearer
    // sense.
    let (_, still) = joint_feature(ir, "Still");
    assert_eq!(still.kind, JointKind::Parallel);
    assert_eq!(still.alignment, Some(Alignment::Nearest));
    assert!(option(&still.source_settings, "Angle").is_some());

    // A limit whose flag is off, and a limit the joint type does not use, is
    // a source setting.
    let (_, slide) = joint_feature(ir, "Slide");
    let JointKind::Motion {
        motion: JointMotion::Slider { axis: FrameAxis::Z },
        limits,
        ..
    } = &slide.kind
    else {
        panic!("{:?}", slide.kind);
    };
    let [AxisLimits::Translation {
        limits: Some(bounds),
        ..
    }] = limits.as_slice()
    else {
        panic!("{limits:?}");
    };
    assert_eq!(bounds.min, None);
    assert!(matches!(bounds.max, Some(Dimension::Literal(length)) if length.get() == 9.0));
    assert_eq!(slide.alignment, Some(Alignment::Nearest));
    for name in ["LengthMin", "EnableLengthMin", "AngleMin", "EnableAngleMin"] {
        assert!(option(&slide.source_settings, name).is_some(), "{name}");
    }
    for name in ["LengthMax", "EnableLengthMax"] {
        assert_eq!(option(&slide.source_settings, name), None, "{name}");
    }
    assert!(
        slide.native_options.is_empty(),
        "{:?}",
        slide.native_options
    );

    // A detached connector picks the referenced occurrence, moved by its
    // stored frame; its elements and inert offset stay native.
    let (_, held) = joint_feature(ir, "Held");
    let bracket = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence.native_ref.as_deref() == Some("fcstd:native:object#Bracket")
                && !occurrence.id.as_str().ends_with(":container")
        })
        .unwrap()
        .id
        .clone();
    assert_eq!(
        frame_picks(&held.connectors[0]),
        [(None, vec![bracket.as_str().to_owned()])]
    );
    let first = frame(&held.connectors[0]);
    let [PostStep::Fixed(step)] = first.after.as_slice() else {
        panic!("{:?}", first.after);
    };
    assert_eq!(step.affine_rows()[0][3], 3.0);
    assert_eq!(first.resolved.as_ref().unwrap().origin.get().x, 3.0);
    let text = |name: &str| match option(&held.native_options, name) {
        Some(NativeOption::Text(Dimension::Literal(text))) => text.as_str().to_owned(),
        other => panic!("{name}: {other:?}"),
    };
    assert_eq!(text("Reference1.sub0"), "Bracket.Face1");
    assert_eq!(text("Reference1.sub1"), "Bracket.Edge2");
    let real = |name: &str| match option(&held.native_options, name) {
        Some(NativeOption::Real(Dimension::Literal(value))) => value.get(),
        other => panic!("{name}: {other:?}"),
    };
    assert_eq!(
        ["px", "py", "pz", "qx", "qy", "qz", "qw"]
            .map(|component| real(&format!("Offset1.{component}"))),
        [0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 1.0]
    );
    assert_eq!(
        option(&held.source_settings, "Detach1"),
        Some(NativeOption::Bool(Dimension::Literal(true)))
    );
    assert_eq!(
        frame_picks(&held.connectors[1]),
        [(
            Some("Edge3".to_owned()),
            vec![ir
                .model
                .occurrences
                .iter()
                .find(|occurrence| {
                    occurrence.native_ref.as_deref() == Some("fcstd:native:object#Pin")
                        && !occurrence.id.as_str().ends_with(":container")
                })
                .unwrap()
                .id
                .as_str()
                .to_owned()]
        )]
    );

    // A joint that stores no second reference is a loss, not a refusal.
    assert!(ir
        .design
        .features
        .iter()
        .all(|feature| feature.name.as_deref() != Some("Loose")));
    let unresolved = crate::loss::FreecadLossCode::FeatureJointUnresolved.note("");
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == unresolved.code && loss.message.contains("Loose")),
        "{:?}",
        decoded.report().losses
    );
}

const JOINT_EXPRESSIONS: &str = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="13">
 <Object type="Assembly::AssemblyObject" name="Assembly" id="1"/>
 <Object type="Part::Feature" name="BracketShape" id="2"/>
 <Object type="Part::Feature" name="PinShape" id="3"/>
 <Object type="App::Link" name="Bracket" id="4"/>
 <Object type="App::Link" name="Pin" id="5"/>
 <Object type="Spreadsheet::Sheet" name="Sheet" id="6"/>
 <Object type="App::FeaturePython" name="Gap" id="7"/>
 <Object type="App::FeaturePython" name="Turn" id="8"/>
 <Object type="App::FeaturePython" name="Wrapped" id="9"/>
 <Object type="App::FeaturePython" name="Held" id="10"/>
 <Object type="App::FeaturePython" name="Limited" id="11"/>
 <Object type="App::FeaturePython" name="Geared" id="12"/>
 <Object type="App::FeaturePython" name="Spring" id="13"/>
</Objects>
<ObjectData Count="13">
 <Object name="Assembly"><Properties Count="1"><Property name="Group" type="App::PropertyLinkList"><LinkList count="9"><Link value="Bracket"/><Link value="Pin"/><Link value="Gap"/><Link value="Turn"/><Link value="Wrapped"/><Link value="Held"/><Link value="Limited"/><Link value="Geared"/><Link value="Spring"/></LinkList></Property></Properties></Object>
 <Object name="BracketShape"><Properties Count="0"/></Object>
 <Object name="PinShape"><Properties Count="0"/></Object>
 <Object name="Bracket"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="BracketShape"/></Property></Properties></Object>
 <Object name="Pin"><Properties Count="1"><Property name="LinkedObject" type="App::PropertyXLink"><XLink file="" name="PinShape"/></Property></Properties></Object>
 <Object name="Sheet"><Properties Count="1"><Property name="cells" type="Spreadsheet::PropertySheet"><Cells Count="3"><Cell address="A1" alias="w" content="12"/><Cell address="A2" alias="a" content="30"/><Cell address="A3" alias="twice" content="=Gap.Distance * 2"/></Cells></Property></Properties></Object>
 <Object name="Gap"><Properties Count="5">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="2" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Distance" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Distance" expression="Sheet.w"/></ExpressionEngine></Property>
 </Properties></Object>
 <Object name="Turn"><Properties Count="5">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="30"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Angle" expression="Sheet.a"/></ExpressionEngine></Property>
 </Properties></Object>
 <Object name="Wrapped"><Properties Count="5">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Angle" type="App::PropertyAngle"><Float value="-330"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Angle" expression="Sheet.a - 360"/></ExpressionEngine></Property>
 </Properties></Object>
 <Object name="Held"><Properties Count="7">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="0" CustomEnum="true"/><CustomEnumList count="4"><Enum value="Fixed"/><Enum value="Angle"/><Enum value="Distance"/><Enum value="Slider"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Offset1" type="App::PropertyPlacement"><PropertyPlacement Px="0" Py="0" Pz="12" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
  <Property name="Detach1" type="App::PropertyBool"><Bool value="false"/></Property>
  <Property name="Visibility" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="3"><Expression path=".Offset1.Base.z" expression="Sheet.w"/><Expression path="Detach1" expression="Sheet.w &gt; 20"/><Expression path="Visibility" expression="Sheet.w &gt; 10"/></ExpressionEngine></Property>
 </Properties></Object>
 <Object name="Limited"><Properties Count="6">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="Fixed"/><Enum value="Revolute"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="AngleMax" type="App::PropertyAngle"><Float value="45"/></Property>
  <Property name="EnableAngleMax" type="App::PropertyBool"><Bool value="true"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="AngleMax" expression="Sheet.a + 15"/></ExpressionEngine></Property>
 </Properties></Object>
 <Object name="Geared"><Properties Count="6">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="Fixed"/><Enum value="Gears"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Distance" type="App::PropertyLength"><Float value="10"/></Property>
  <Property name="Distance2" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Distance2" expression="Sheet.w"/></ExpressionEngine></Property>
 </Properties></Object>
 <Object name="Spring"><Properties Count="5">
  <Property name="JointType" type="App::PropertyEnumeration"><Integer value="1" CustomEnum="true"/><CustomEnumList count="2"><Enum value="Fixed"/><Enum value="Spring"/></CustomEnumList></Property>
  <Property name="Reference1" type="App::PropertyXLinkSub"><XLink file="" name="Bracket" sub="Face1"/></Property>
  <Property name="Reference2" type="App::PropertyXLinkSub"><XLink file="" name="Pin" sub="Face2"/></Property>
  <Property name="Distance" type="App::PropertyLength"><Float value="12"/></Property>
  <Property name="ExpressionEngine" type="App::PropertyExpressionEngine"><ExpressionEngine count="1"><Expression path="Distance" expression="Sheet.w"/></ExpressionEngine></Property>
 </Properties></Object>
</ObjectData></Document>"#;

#[test]
fn a_driven_joint_value_binds_a_parameter_of_the_joint() {
    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::ops::shared::{AxisLimits, Bounds, Coupling, JointKind};
    use cadmpeg_ir::design::options::NativeOption;

    let decoded = FcstdCodec
        .decode(
            &mut Cursor::new(archive(JOINT_EXPRESSIONS)),
            &DecodeOptions {
                depth: DecodeDepth::Design,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let ir = decoded.ir();
    assert_valid_document(ir);
    let parameter = |id: &str| {
        ir.design
            .parameters
            .iter()
            .find(|parameter| parameter.id.as_str() == id)
            .unwrap_or_else(|| panic!("{id}: {:?}", ir.design.parameters))
    };
    let cell = |alias: &str| {
        ir.design
            .parameters
            .iter()
            .find(|parameter| {
                parameter
                    .name
                    .as_ref()
                    .is_some_and(|name| name.as_str() == alias)
            })
            .unwrap_or_else(|| panic!("{alias}: {:?}", ir.design.parameters))
            .id
            .as_str()
            .to_owned()
    };
    let neutral = |id: &str| {
        let wire = json::to_value(&parameter(id).definition).unwrap();
        assert_eq!(wire["value"]["kind"], "neutral", "{id}: {wire}");
        let refs = wire["value"]["value"]["refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|reference| reference["target"]["value"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        (
            wire["value"]["value"]["text"].as_str().unwrap().to_owned(),
            refs,
        )
    };
    let unbound_all = |name: &str| {
        let code = crate::loss::FreecadLossCode::FeatureJointValueUnbound.note("");
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == code.code && loss.message.contains(&format!("#{name} ")))
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>()
    };
    let unbound = |name: &str| unbound_all(name).into_iter().next();
    let owned_by = |id: &str, joint: &cadmpeg_ir::design::feature::Feature| {
        assert_eq!(
            parameter(id).owner,
            Some(ParameterOwner::Feature(joint.id.clone())),
            "{id}"
        );
    };

    // The distance of a planar joint is the stored distance, bound to the
    // parameter that the joint's expression states.
    let (gap_feature, gap) = joint_feature(ir, "Gap");
    let JointKind::Motion { offset, .. } = &gap.kind else {
        panic!("{:?}", gap.kind);
    };
    let gap_id = "fcstd:design:parameter#Gap:Distance";
    assert!(
        matches!(&offset.z, Some(Dimension::Parameter(id)) if id.as_str() == gap_id),
        "{offset:?}"
    );
    owned_by(gap_id, gap_feature);
    assert_eq!(neutral(gap_id), ("$0 * 1[mm]".to_owned(), vec![cell("w")]));
    assert_eq!(unbound("Gap"), None);

    // An angle in a half turn is the stored angle.
    let (turn_feature, turn) = joint_feature(ir, "Turn");
    let turn_id = "fcstd:design:parameter#Turn:Angle";
    assert!(
        matches!(&turn.kind, JointKind::Angle { angle: Some(Dimension::Parameter(id)), .. } if id.as_str() == turn_id),
        "{:?}",
        turn.kind
    );
    owned_by(turn_id, turn_feature);
    assert_eq!(
        neutral(turn_id),
        ("$0 * 1[deg]".to_owned(), vec![cell("a")])
    );
    assert_eq!(unbound("Turn"), None);

    // The solver reflects an angle outside a half turn: the joint states
    // the reflected angle, and the stored angle is a source setting bound
    // to the parameter.
    let (wrapped_feature, wrapped) = joint_feature(ir, "Wrapped");
    let wrapped_id = "fcstd:design:parameter#Wrapped:Angle";
    assert!(
        matches!(
            &wrapped.kind,
            JointKind::Angle {
                angle: Some(Dimension::Literal(_)),
                ..
            }
        ),
        "{:?}",
        wrapped.kind
    );
    assert!(
        matches!(
            option(&wrapped.source_settings, "Angle"),
            Some(NativeOption::Angle(Dimension::Parameter(id))) if id.as_str() == wrapped_id
        ),
        "{:?}",
        wrapped.source_settings
    );
    owned_by(wrapped_id, wrapped_feature);
    assert!(
        unbound("Wrapped").is_some_and(|message| message.contains("Angle")),
        "{:?}",
        decoded.report().losses
    );

    // A driven placement and flag keep their stored values and are a loss;
    // a driven property the joint does not state is a loss that says so.
    let messages = unbound_all("Held");
    let [stated, unstated] = messages.as_slice() else {
        panic!("{:?}", decoded.report().losses);
    };
    assert!(stated.contains("states Detach1, Offset1 "), "{stated}");
    assert!(
        unstated.contains("entries on Visibility, which the joint does not state"),
        "{unstated}"
    );
    assert!(ir
        .design
        .parameters
        .iter()
        .all(|parameter| !parameter.id.as_str().contains("Held")));

    // A driven enabled limit is its parameter.
    let (_, limited) = joint_feature(ir, "Limited");
    let limited_id = "fcstd:design:parameter#Limited:AngleMax";
    let JointKind::Motion { limits, .. } = &limited.kind else {
        panic!("{:?}", limited.kind);
    };
    assert!(
        matches!(
            limits.as_slice(),
            [AxisLimits::Rotation {
                limits: Some(Bounds {
                    min: None,
                    max: Some(Dimension::Parameter(id)),
                }),
                ..
            }] if id.as_str() == limited_id
        ),
        "{limits:?}"
    );
    assert_eq!(unbound("Limited"), None);

    // A driven second gear radius is its parameter.
    let (_, geared) = joint_feature(ir, "Geared");
    let geared_id = "fcstd:design:parameter#Geared:Distance2";
    assert!(
        matches!(
            &geared.kind,
            JointKind::Coupling {
                coupling: Coupling::Gear {
                    radii: [Some(Dimension::Literal(_)), Some(Dimension::Parameter(id))],
                    ..
                },
                ..
            } if id.as_str() == geared_id
        ),
        "{:?}",
        geared.kind
    );
    assert_eq!(unbound("Geared"), None);

    // A native family keeps a driven value as a native option bound to its
    // parameter.
    let (_, spring) = joint_feature(ir, "Spring");
    let spring_id = "fcstd:design:parameter#Spring:Distance";
    assert!(
        matches!(
            option(&spring.native_options, "Distance"),
            Some(NativeOption::Length(Dimension::Parameter(id))) if id.as_str() == spring_id
        ),
        "{:?}",
        spring.native_options
    );
    assert_eq!(unbound("Spring"), None);

    // A cell that references a joint value references the joint's
    // parameter.
    let twice = cell("twice");
    assert_eq!(
        parameter(&twice).kind,
        cadmpeg_ir::design::units::ValueKind::Quantity(cadmpeg_ir::design::units::Quantity::LENGTH)
    );
    assert_eq!(
        neutral(&twice),
        ("$0 * 2".to_owned(), vec![gap_id.to_owned()])
    );
}
