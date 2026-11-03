// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{AssemblyJoint, JointConnector, JointId, JointOperand, PairedJointKind};
use cadmpeg_ir::transform::Transform;

#[test]
fn an_omitted_nested_placement_keeps_an_unbound_joint_native_and_its_feature_valid() {
    let id = OccurrenceId::mint("f3d:model:occurrence#member").unwrap();
    let occurrence = serde_json::from_value(serde_json::json!({
        "id": id, "prototype": {"scope":"unresolved"}, "parent": {"kind":"root"},
        "ordinal":0, "transform":Transform::identity(), "scale":[1.0,1.0,1.0]
    }))
    .unwrap();
    let joint = |key: &str, residual: Vec<String>| {
        let connector = |operand| JointConnector {
            operand,
            frame: Transform::identity(),
            detached: false,
        };
        AssemblyJoint::paired(
            JointId::mint(format!("f3d:model:joint#{key}")).unwrap(),
            PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [
                connector(JointOperand::occurrence(id.clone(), "member", residual)),
                connector(JointOperand::root("root", Vec::new())),
            ],
            None,
        )
    };
    let omitted = joint("omitted", vec!["omitted-child-key".to_owned()]);
    let bound = joint("bound", Vec::new());
    let feature = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#joint").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Assemble".to_owned()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::AssemblyJoint {
                joint: omitted.id.clone(),
            }),
        ),
        native_ref: None,
    };
    let mut model = cadmpeg_ir::document::Model::default();
    model.occurrences.push(occurrence);
    model.features.push(feature);
    let mut external = joint("external", Vec::new());
    external.connectors_mut().next().unwrap().operand = JointOperand::external(
        cadmpeg_ir::products::ExternalDocument::document_id("unbound-document"),
        "external-member",
        Vec::new(),
    );
    model.assembly_joints = vec![omitted, bound.clone(), external];
    let mut unchanged = model.clone();
    let mut losses = Vec::new();
    super::super::omit_unbound_member_joints(
        &mut unchanged,
        &std::collections::BTreeSet::new(),
        false,
        &mut losses,
    );
    assert_eq!(unchanged.assembly_joints, model.assembly_joints);
    assert!(losses.is_empty());
    for root in [false, true] {
        let mut projected = model.clone();
        let members = if root {
            std::collections::BTreeSet::new()
        } else {
            std::collections::BTreeSet::from([id.clone()])
        };
        let mut losses = Vec::new();
        super::super::omit_unbound_member_joints(&mut projected, &members, root, &mut losses);
        assert_eq!(projected.assembly_joints, [bound.clone()]);
        assert!(matches!(
            projected.features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        assert_eq!(losses.len(), 2);
        assert!(losses
            .iter()
            .all(|loss| loss.code == crate::loss::F3dLossCode::AssemblyJointUnprojected.kind()));
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model = projected;
        let validation = cadmpeg_ir::validate_neutral(&ir, losses).unwrap();
        assert!(validation.is_ok(), "{validation:#?}");
    }
}
