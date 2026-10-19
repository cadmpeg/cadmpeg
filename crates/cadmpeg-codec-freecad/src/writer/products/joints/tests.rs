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
fn local_joint_reference_absent_from_current_document_is_a_precondition() {
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
    let error = write(&ir, &mut Vec::new(), &mut HashMap::new(), &mut Vec::new())
        .expect_err("absent operand");
    assert!(matches!(error, CodecError::Malformed(_)));
}
