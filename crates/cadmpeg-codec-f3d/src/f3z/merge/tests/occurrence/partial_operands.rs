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
                ExternalDocument::default(),
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
            ExternalDocument::default(),
            "f3d:Design/BulkStream.dat:design-parameter-scope#30",
            vec!["construction-record#10".to_owned()],
        );
    }
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
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

#[test]
fn form_occurrences_rewrite_every_typed_identity_with_absent_native_targets() {
    use cadmpeg_ir::document::EntityRewrite;
    use cadmpeg_ir::features::form::*;
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureEvaluation, FeatureOperation, NonEmptyMembers,
    };
    use cadmpeg_ir::ids::{BodyId, NativeId, SubdId};
    let mut feature = super::feature("f3d:model:feature#form", 0);
    let native = NativeId::mint("f3d:Design1/BulkStream.dat:design-record#99").unwrap();
    let resource = FormResource {
        source: native.clone(),
        entry: cadmpeg_core::nonblank_literal!("Design/TSplines.BlobParts/cage.tsm"),
    };
    let construction = FormConstruction::try_new(
        FormInputs {
            parameters: std::collections::BTreeMap::from([(
                cadmpeg_core::nonblank_literal!("width"),
                "width / 2".into(),
            )]),
            references: vec![native.clone()],
            lists: vec![FormNativeList {
                source: FormInputPath {
                    references: NonEmptyMembers::try_from(vec![native.clone()]).unwrap(),
                },
                role: FormListRole::CageObjects,
                declared_count: 1,
                members: vec![FormNativeMember {
                    ordinal: 0,
                    reference: native.clone(),
                }],
            }],
            ..FormInputs::default()
        },
        vec![
            FormCageObservation {
                paths: vec![FormInputPath {
                    references: NonEmptyMembers::try_from(vec![native.clone()]).unwrap(),
                }],
                binding: FormCageBinding::Decoded {
                    cage: SubdId::mint("f3d:model:subd#member").unwrap(),
                    resource: Some(resource.clone()),
                },
            },
            FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Decoded {
                    cage: SubdId::mint("f3d:model:subd#candidate").unwrap(),
                    resource: None,
                },
            },
            FormCageObservation {
                paths: Vec::new(),
                binding: FormCageBinding::Undecoded {
                    resource: FormResource {
                        source: resource.source,
                        entry: cadmpeg_core::nonblank_literal!(
                            "Design/TSplines.BlobParts/other.tsm"
                        ),
                    },
                    availability: FormResourceAvailability::ActiveUndecoded,
                },
            },
        ],
        FormMembership::Partial {
            cardinality: Some(2),
            order: FormOrder::Unknown,
            members: vec![FormMember {
                observation: FormObservationIndex(0),
                ordinal: Some(0),
            }],
        },
    )
    .unwrap();
    feature.evaluation = FeatureEvaluation::new(
        FeatureDefinition::Operation(FeatureOperation::Form(construction)),
        vec![BodyId::mint("f3d:model:body#result").unwrap()]
            .try_into()
            .unwrap(),
    );
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    for occurrence in ["left", "right"] {
        let rewritten = OccurrenceScope {
            ctx: &ctx,
            occurrence,
            native_text: &TextReplacements::default(),
        }
        .rewrite(feature.clone())
        .unwrap();
        assert_eq!(
            rewritten.id.as_str(),
            format!("f3d:xref/{occurrence}/model:feature#form")
        );
        assert_eq!(
            rewritten.evaluation.outputs()[0].as_str(),
            format!("f3d:xref/{occurrence}/model:body#result")
        );
        let FeatureDefinition::Operation(FeatureOperation::Form(construction)) =
            rewritten.evaluation.definition()
        else {
            panic!("Form");
        };
        assert_eq!(construction.inputs().parameters["width"], "width / 2");
        let expected = format!("f3d:xref/{occurrence}/Design1/BulkStream.dat:design-record#99");
        assert_eq!(construction.inputs().references[0].as_str(), expected);
        assert_eq!(
            construction.inputs().lists[0].members[0].reference.as_str(),
            expected
        );
        assert_eq!(
            construction.inputs().lists[0].source.references[0].as_str(),
            expected
        );
        assert_eq!(
            construction.observations()[0].paths[0].references[0].as_str(),
            expected
        );
        let FormCageBinding::Decoded {
            resource: Some(resource),
            ..
        } = &construction.observations()[0].binding
        else {
            panic!("resource");
        };
        assert_eq!(resource.source.as_str(), expected);
        assert_eq!(
            resource.entry.as_str(),
            "Design/TSplines.BlobParts/cage.tsm"
        );
        assert_eq!(
            construction
                .referenced_cages()
                .map(SubdId::as_str)
                .collect::<Vec<_>>(),
            [
                format!("f3d:xref/{occurrence}/model:subd#member"),
                format!("f3d:xref/{occurrence}/model:subd#candidate")
            ]
        );
        assert_eq!(construction.committed_cages().count(), 1);
        let FormCageBinding::Undecoded {
            resource,
            availability,
        } = &construction.observations()[2].binding
        else {
            panic!("undecoded");
        };
        assert_eq!(resource.source.as_str(), expected);
        assert_eq!(*availability, FormResourceAvailability::ActiveUndecoded);
        assert!(
            matches!(construction.membership(), FormMembership::Partial { cardinality: Some(2), order: FormOrder::Unknown, members } if members == &[FormMember { observation: FormObservationIndex(0), ordinal: Some(0) }])
        );
    }
}
