use super::standalone_joint_model;
use crate::f3z::merge::{self, OccurrenceScope};
use cadmpeg_ir::schema::rewrite::typed::TextReplacements;

#[test]
fn partial_connector_operands_survive_standalone_and_archive_rebinding() {
    use cadmpeg_ir::products::{ExternalDocument, JointOperand};
    for mode in [merge::JointRebind::CompleteScopes, merge::JointRebind::All] {
        let (mut model, _) = standalone_joint_model(Vec::new());
        for (ordinal, connector) in model.assembly_joints[0].connectors_mut().enumerate() {
            connector.operand = JointOperand::external(
                ExternalDocument::Missing {},
                "f3d:scope#standalone",
                vec![format!("construction-record#{}", ordinal + 10)],
            );
        }
        crate::design::assembly::bind_assembly_joint_features(
            &mut model.features,
            &model.assembly_joints,
        );
        let joints = model.assembly_joints.clone();
        let features = model.features.clone();
        let children = std::collections::BTreeMap::new();
        let mut losses = Vec::new();
        merge::rebind_joints(
            &mut merge::JointRows {
                occurrences: &model.occurrences,
                joints: &mut model.assembly_joints,
                features: &mut model.features,
            },
            &children,
            &children,
            &std::collections::BTreeSet::new(),
            mode,
            &mut losses,
        );
        assert_eq!(model.assembly_joints, joints);
        assert_eq!(model.features, features);
        assert!(losses.is_empty());
    }
}

#[test]
fn partial_connector_objects_follow_their_archive_source_scope() {
    use cadmpeg_ir::document::EntityRewrite;
    use cadmpeg_ir::products::{ExternalDocument, JointOperand};
    let (mut model, _) = standalone_joint_model(Vec::new());
    model.assembly_joints[0].native_ref =
        Some("f3d:Design/BulkStream.dat:design-parameter-scope#30".to_owned());
    for connector in model.assembly_joints[0].connectors_mut() {
        connector.operand = JointOperand::external(
            ExternalDocument::Missing {},
            "f3d:Design/BulkStream.dat:design-parameter-scope#30",
            vec!["construction-record#10".to_owned()],
        );
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    for occurrence in ["first", "second"] {
        let expected =
            format!("f3d:xref/{occurrence}/Design/BulkStream.dat:design-parameter-scope#30");
        let targets = std::collections::BTreeMap::from([(
            "f3d:Design/BulkStream.dat:design-parameter-scope#30".to_owned(),
            expected.clone(),
        )]);
        let native_text = TextReplacements::build(targets.iter()).unwrap();
        let mut scope = OccurrenceScope {
            native_text: &native_text,
            ctx: &ctx,
            occurrence,
        };
        let joint = scope.rewrite(model.assembly_joints[0].clone()).unwrap();
        assert_eq!(joint.native_ref.as_deref(), Some(expected.as_str()));
        for connector in joint.connectors() {
            assert_eq!(connector.operand.object, expected);
            assert_eq!(connector.operand.subelements, ["construction-record#10"]);
        }
    }
}
