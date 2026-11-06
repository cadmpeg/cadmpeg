// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_ir::products::{AssemblyJoint, JointConnector, JointId};

#[test]
fn paired_joint_emits_current_family_limits_and_connector_references() {
    let mut ir = CadIr::empty();
    let connector = |name: &str| JointConnector {
        operand: JointOperand::root(name, vec!["Face1".into()]),
        frame: Transform::identity(),
        detached: false,
    };
    let joint = AssemblyJoint::paired(
        JointId::mint("authored:model:joint#Hinge").expect("id"),
        PairedJointKind::Revolute {
            angle: FiniteReal::new(std::f64::consts::FRAC_PI_2),
            angular_limits: JointLimits::new(
                Some(-std::f64::consts::PI),
                Some(std::f64::consts::PI),
            ),
        },
        [connector("base"), connector("arm")],
        None,
    );
    ir.model.assembly_joints.push(joint);
    let mut objects = vec![
        Object::new("Base".into(), "Part::Feature", "Base"),
        Object::new("Arm".into(), "Part::Feature", "Arm"),
    ];
    let mut names = HashMap::from([("base".into(), "Base".into()), ("arm".into(), "Arm".into())]);
    let mut losses = Vec::new();
    write(&ir, &mut objects, &mut names, &mut losses).expect("joint");
    let joint = objects.last().expect("joint output");
    assert!(joint.properties["JointType"].contains("value=\"Revolute\""));
    assert!(joint.properties["Angle"].contains("value=\"90\""));
    assert!(joint.properties["AngleMin"].contains("value=\"-180\""));
    assert!(joint.properties["AngleMax"].contains("value=\"180\""));
    let reference =
        roxmltree::Document::parse(&joint.properties["Reference2"]).expect("reference XML");
    assert_eq!(
        reference
            .descendants()
            .find(|node| node.has_tag_name("XLink"))
            .expect("link")
            .attribute("name"),
        Some("Arm")
    );
    assert_eq!(
        reference
            .descendants()
            .find(|node| node.has_tag_name("Sub"))
            .expect("selection")
            .attribute("value"),
        Some("Face1")
    );
    assert!(losses.is_empty());
}

#[test]
fn overflowing_grounded_joint_offset_keeps_its_frame() {
    let large_translation = Transform::affine([
        [1.0, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    let connector = JointConnector {
        operand: JointOperand::root("part", Vec::new()),
        frame: large_translation,
        detached: false,
    };
    let mut ir = CadIr::empty();
    ir.model.assembly_joints.push(AssemblyJoint::grounded(
        JointId::mint("authored:model:joint#Ground").unwrap(),
        connector,
        Some(large_translation),
    ));
    let mut objects = vec![Object::new("Part".into(), "Part::Feature", "Part")];
    let mut names = HashMap::from([("part".into(), "Part".into())]);
    let mut losses = Vec::new();
    write(&ir, &mut objects, &mut names, &mut losses).unwrap();
    let document =
        roxmltree::Document::parse(&objects.last().unwrap().properties["Placement"]).unwrap();
    let placement = document
        .descendants()
        .find(|node| node.has_tag_name("PropertyPlacement"))
        .unwrap();
    assert_eq!(
        placement.attribute("Px").unwrap().parse::<f64>().unwrap(),
        f64::MAX
    );
    assert!(losses.iter().any(|loss| loss
        .message
        .contains("attachment offset cannot be composed")));
}

#[test]
fn missing_grounded_joint_target_is_reported() {
    let mut ir = CadIr::empty();
    let connector = JointConnector {
        operand: JointOperand::root("deleted", Vec::new()),
        frame: Transform::identity(),
        detached: false,
    };
    ir.model.assembly_joints.push(AssemblyJoint::grounded(
        JointId::mint("authored:model:joint#Ground").expect("id"),
        connector,
        None,
    ));
    let mut objects = Vec::new();
    let mut losses = Vec::new();
    write(&ir, &mut objects, &mut HashMap::new(), &mut losses).unwrap();
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("authored:model:joint#Ground")));
    assert!(!objects[0].properties.contains_key("ObjectToGround"));
}

#[test]
fn missing_joint_operand_keeps_the_other_reference() {
    let mut ir = CadIr::empty();
    let connector = |name: &str| JointConnector {
        operand: JointOperand::root(name, Vec::new()),
        frame: Transform::identity(),
        detached: false,
    };
    ir.model.assembly_joints.push(AssemblyJoint::paired(
        JointId::mint("authored:model:joint#Pair").unwrap(),
        PairedJointKind::Revolute {
            angle: None,
            angular_limits: None,
        },
        [connector("missing"), connector("present")],
        None,
    ));
    let mut objects = vec![Object::new("Present".into(), "Part::Feature", "Present")];
    let mut names = HashMap::from([("present".into(), "Present".into())]);
    let mut losses = Vec::new();
    write(&ir, &mut objects, &mut names, &mut losses).unwrap();
    let joint = objects.last().unwrap();
    assert!(!joint.properties.contains_key("Reference1"));
    assert!(joint.properties["Reference2"].contains("Present"));
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("authored:model:joint#Pair")));
}

#[test]
fn nested_occurrence_reference_keeps_the_complete_object_path() {
    let occurrence = "authored:model:occurrence#Outer".try_into().expect("id");
    let operand = JointOperand::nested_occurrence(
        occurrence,
        vec!["Inner".into(), "Leaf".into()],
        "Part",
        vec!["Face1".into(), "Vertex2".into()],
    );
    let names = HashMap::from([("authored:model:occurrence#Outer".into(), "Outer".into())]);
    let mut losses = Vec::new();
    let xml = reference(&operand, &names, &mut losses).expect("reference");
    let document = roxmltree::Document::parse(&xml).expect("XML");
    assert_eq!(document.root_element().attribute("name"), Some("Outer"));
    let selections: Vec<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("Sub"))
        .map(|node| node.attribute("value").expect("selection"))
        .collect();
    assert_eq!(
        selections,
        ["Inner.Leaf.Part.Face1", "Inner.Leaf.Part.Vertex2"]
    );
    assert!(losses.is_empty());
}

#[test]
fn unsupported_joint_families_report_each_omission_and_keep_supported_siblings() {
    use cadmpeg_ir::products::{PlaneSense, TangentContact};

    let unsupported = [
        PairedJointKind::PlaneCoincidence {
            sense: PlaneSense::Aligned,
            offset: FiniteReal::new(2.0),
        },
        PairedJointKind::AxisCoincidence {},
        PairedJointKind::PointOnAxis {
            distance: FiniteReal::new(3.0),
        },
        PairedJointKind::Insert {
            sense: PlaneSense::Aligned,
            offset: FiniteReal::new(4.0),
        },
        PairedJointKind::Tangent {
            contact: TangentContact::Outside,
        },
    ];
    let mut ir = CadIr::empty();
    let connector = || JointConnector {
        operand: JointOperand::root("part", Vec::new()),
        frame: Transform::identity(),
        detached: false,
    };
    for (index, kind) in unsupported.into_iter().enumerate() {
        ir.model.assembly_joints.push(AssemblyJoint::paired(
            JointId::mint(format!("authored:model:joint#Omitted{index}")).expect("id"),
            kind,
            [connector(), connector()],
            None,
        ));
    }
    ir.model.assembly_joints.push(AssemblyJoint::paired(
        JointId::mint("authored:model:joint#Ball").expect("id"),
        PairedJointKind::Ball {},
        [connector(), connector()],
        None,
    ));
    let mut objects = vec![Object::new("Part".into(), "Part::Feature", "Part")];
    let mut names = HashMap::from([("part".into(), "Part".into())]);
    let mut losses = Vec::new();
    write(&ir, &mut objects, &mut names, &mut losses).expect("joint write");
    assert_eq!(objects.len(), 2);
    assert!(objects[1].properties["JointType"].contains("value=\"Ball\""));
    assert_eq!(losses.len(), 5);
    for (index, loss) in losses.iter().enumerate() {
        assert_eq!(loss.code.local_code(), "feature.not-transferred");
        assert!(loss
            .message
            .contains(&format!("authored:model:joint#Omitted{index}")));
    }
}
