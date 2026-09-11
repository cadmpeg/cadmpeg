// SPDX-License-Identifier: Apache-2.0

use crate::decode::feature_completeness::{
    combine_definition_is_incomplete, incomplete_expression_parameters,
};

use cadmpeg_ir::math::Point2;

use super::*;
use crate::native::om::display_color::{
    DisplayColorFrame, RmDisplayColorAssignment, RmDisplayColorAssignmentEncoding,
};
use crate::om::column_row::{LinkedRow, TargetRow};
use crate::om::compact::CompactIndexAtom;

#[test]
fn rm_source_color_bindings_require_one_palette_per_source_identity() {
    let assignment = |id: &str, source_id: Option<&str>, color_definition: &str, offset: u64| {
        RmDisplayColorAssignment {
            id: id.into(),
            ordinal: 0,
            frame: DisplayColorFrame::new(
                RmDisplayColorAssignmentEncoding::Target(
                    TargetRow::<(), u64>::new(
                        CompactIndexAtom::read(&[7]).unwrap().into(),
                        [1, 2, 3].map(|value| CompactIndexAtom::read(&[value]).unwrap().into()),
                        crate::om::discriminators::IndexRowMode::Form04,
                        offset + 2,
                    )
                    .unwrap(),
                ),
                crate::om::color::PaletteIndex::new(201).unwrap(),
            )
            .unwrap(),
            target_object_id: source_id.map(str::to_owned),
            color_definition: color_definition.into(),
            source_entry: "/Root/FastLoad/RMFastLoad".into(),
        }
    };
    let assignments = [
        assignment("assignment-b", Some("source-a"), "color-a", 20),
        assignment("assignment-a", Some("source-a"), "color-a", 10),
        assignment("assignment-c", Some("source-b"), "color-a", 30),
        assignment("assignment-d", Some("source-c"), "color-a", 40),
        assignment("assignment-e", Some("source-c"), "color-b", 50),
        assignment("assignment-f", None, "color-a", 60),
    ];
    assert_eq!(
        resolve_rm_source_color_bindings(&assignments),
        vec![
            RmSourceColorBinding {
                source_id: "source-a".into(),
                color_definition: "color-a".into(),
                source_offset: 10,
            },
            RmSourceColorBinding {
                source_id: "source-b".into(),
                color_definition: "color-a".into(),
                source_offset: 30,
            },
        ]
    );
}
#[test]
fn ungrouped_simple_holes_follow_authoritative_history_order() {
    use crate::native::features::holes::FeatureSimpleHoleConstructionGroup;
    use crate::native::features::holes::FeatureSimpleHoleTemplate;
    use crate::native::features::holes::SimpleHoleEndTreatment;
    use crate::native::features::holes::SimpleHoleExtent;
    use crate::native::features::holes::SimpleHoleFamily;
    use crate::native::features::holes::SimpleHoleForm;

    let template = |operation_label: &str| FeatureSimpleHoleTemplate {
        id: format!("template-{operation_label}"),
        operation_label: operation_label.to_string(),
        payload_string: format!("payload-{operation_label}"),
        family: SimpleHoleFamily::GeneralHole,
        form: SimpleHoleForm::Simple,
        extent: SimpleHoleExtent::Through,
        start_treatment: SimpleHoleEndTreatment::Chamfer,
        end_treatment: SimpleHoleEndTreatment::Chamfer,
    };
    let templates = vec![template("operation#newer"), template("operation#older")];
    let operation_positions =
        BTreeMap::from([("operation#older", 0usize), ("operation#newer", 1usize)]);
    assert_eq!(
        simple_hole_operations(&templates, &[], &operation_positions),
        Some(vec!["operation#older".into(), "operation#newer".into()])
    );

    let unordered_group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: ["a".into(), "b".into()],
        second_data_blocks: ["c".into(), "d".into()],
        members: crate::native::features::holes::SimpleHoleConstructionMembers::new(vec![
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: "operation#newer".into(),
                scalar_lane: "lane-newer".into(),
                block_reference: "blocks-newer".into(),
            },
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: "operation#older".into(),
                scalar_lane: "lane-older".into(),
                block_reference: "blocks-older".into(),
            },
        ])
        .unwrap(),
    };
    assert!(
        simple_hole_operations(&templates, &[unordered_group], &operation_positions,).is_none()
    );

    let mut blind_template = template("operation#blind");
    blind_template.extent = SimpleHoleExtent::Blind;
    blind_template.start_treatment = SimpleHoleEndTreatment::None;
    blind_template.end_treatment = SimpleHoleEndTreatment::None;
    let mixed_templates = vec![
        templates[0].clone(),
        blind_template.clone(),
        templates[1].clone(),
    ];
    let mixed_positions = BTreeMap::from([
        ("operation#older", 0usize),
        ("operation#newer", 1usize),
        ("operation#blind", 2usize),
    ]);
    assert_eq!(
        simple_hole_operations(&mixed_templates, &[], &mixed_positions),
        Some(vec!["operation#older".into(), "operation#newer".into()])
    );
    assert_eq!(
        blind_hole_operations(&mixed_templates, &mixed_positions),
        Some(vec!["operation#blind".into()])
    );
    let duplicate_members =
        crate::native::features::holes::SimpleHoleConstructionMembers::new(vec![
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: "operation#older".into(),
                scalar_lane: "lane-a".into(),
                block_reference: "refs-a".into(),
            },
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: "operation#newer".into(),
                scalar_lane: "lane-b".into(),
                block_reference: "refs-b".into(),
            },
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: "operation#older".into(),
                scalar_lane: "lane-a".into(),
                block_reference: "refs-a".into(),
            },
        ]);
    assert!(duplicate_members.is_err());
}

#[test]
fn exact_hole_package_owns_common_internal_simple_holes() {
    use crate::native::features::holes::FeatureHolePackageConstructionGroupUse;
    use crate::native::features::holes::FeatureSimpleHoleConstructionGroup;
    use crate::native::features::holes::FeatureSimpleHoleTemplate;
    use crate::native::features::holes::SimpleHoleEndTreatment;
    use crate::native::features::holes::SimpleHoleExtent;
    use crate::native::features::holes::SimpleHoleFamily;
    use crate::native::features::holes::SimpleHoleForm;
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::{features::HoleKind, scalar::Length};

    let operations = ["simple-a".to_string(), "simple-b".to_string()];
    let templates = operations
        .iter()
        .map(|operation| FeatureSimpleHoleTemplate {
            id: format!("template-{operation}"),
            operation_label: operation.clone(),
            payload_string: format!("string-{operation}"),
            family: SimpleHoleFamily::GeneralHole,
            form: SimpleHoleForm::Simple,
            extent: SimpleHoleExtent::Through,
            start_treatment: SimpleHoleEndTreatment::Chamfer,
            end_treatment: SimpleHoleEndTreatment::Chamfer,
        })
        .collect::<Vec<_>>();
    let group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: ["a".into(), "b".into()],
        second_data_blocks: ["c".into(), "d".into()],
        members: crate::native::features::holes::SimpleHoleConstructionMembers::new(vec![
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: operations[0].clone(),
                scalar_lane: "lane-a".into(),
                block_reference: "blocks-a".into(),
            },
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: operations[1].clone(),
                scalar_lane: "lane-b".into(),
                block_reference: "blocks-b".into(),
            },
        ])
        .unwrap(),
    };
    let use_ = FeatureHolePackageConstructionGroupUse {
        id: "use".into(),
        operation_label: "package".into(),
        construction_group_lane: "package-lane".into(),
        simple_hole_construction_group: group.id.clone(),
        source_offset: 0,
    };
    let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
    let outputs = operations
        .iter()
        .map(|operation| (operation.clone(), vec![body.clone()]))
        .collect();
    let diameters = operations
        .iter()
        .map(|operation| (operation.clone(), Length::new(5.1).unwrap()))
        .collect();
    let chamfer = HoleKind::Chamfer {
        diameter: cadmpeg_ir::scalar::PositiveLength::new(7.1).unwrap(),
        angle: cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
    };
    let chamfers = operations
        .iter()
        .map(|operation| (operation.clone(), chamfer))
        .collect();

    let projection = super::hole_package_projection(
        &cadmpeg_ir::document::CadIr::empty(),
        &templates,
        std::slice::from_ref(&group),
        std::slice::from_ref(&use_),
        &outputs,
        &diameters,
        &chamfers,
    );
    assert_eq!(
        projection.internal_operations,
        operations.iter().cloned().collect()
    );
    assert_eq!(projection.outputs["package"], std::slice::from_ref(&body));
    assert_eq!(projection.diameters["package"], Length::new(5.1).unwrap());
    assert_eq!(projection.chamfers["package"], chamfer);

    let untreated_templates = templates
        .iter()
        .cloned()
        .map(|mut template| {
            template.start_treatment = SimpleHoleEndTreatment::None;
            template.end_treatment = SimpleHoleEndTreatment::None;
            template
        })
        .collect::<Vec<_>>();
    let projection = super::hole_package_projection(
        &cadmpeg_ir::document::CadIr::empty(),
        &untreated_templates,
        std::slice::from_ref(&group),
        std::slice::from_ref(&use_),
        &outputs,
        &diameters,
        &BTreeMap::new(),
    );
    assert_eq!(
        projection.internal_operations,
        operations.iter().cloned().collect()
    );
    assert_eq!(projection.outputs["package"], [body]);
    assert_eq!(projection.diameters["package"], Length::new(5.1).unwrap());
    assert!(!projection.chamfers.contains_key("package"));

    let mut mixed_templates = untreated_templates.clone();
    mixed_templates[0].start_treatment = SimpleHoleEndTreatment::Chamfer;
    let projection = super::hole_package_projection(
        &cadmpeg_ir::document::CadIr::empty(),
        &mixed_templates,
        std::slice::from_ref(&group),
        std::slice::from_ref(&use_),
        &outputs,
        &diameters,
        &BTreeMap::new(),
    );
    assert!(projection.internal_operations.is_empty());
    assert!(projection.outputs.is_empty());

    let mut mismatched_outputs = outputs;
    mismatched_outputs.insert(
        "simple-b".into(),
        vec![BodyId::mint("test:model:entity#other-body").expect("identity grammar")],
    );
    let projection = super::hole_package_projection(
        &cadmpeg_ir::document::CadIr::empty(),
        &templates,
        std::slice::from_ref(&group),
        std::slice::from_ref(&use_),
        &mismatched_outputs,
        &diameters,
        &chamfers,
    );
    assert!(projection.internal_operations.is_empty());
    assert!(projection.outputs.is_empty());
}

#[test]
fn active_configuration_retains_complete_evaluated_parameter_state() {
    let parameter = |id: &str, ordinal, value, dependencies: Vec<ParameterId>| DesignParameter {
        id: ParameterId::mint(id).expect("identity grammar"),
        owner: None,
        ordinal,
        name: id.into(),
        expression: id.into(),
        display: None,
        value,
        dependencies: (dependencies).try_into().unwrap(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let mut ir = CadIr::empty();
    ir.model.parameters = vec![
        parameter(
            "synthetic:test:id#length",
            0,
            Some(ParameterValue::Length(Length::new(25.4).unwrap())),
            Vec::new(),
        ),
        parameter(
            "synthetic:test:id#angle",
            1,
            Some(ParameterValue::Angle(
                Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            )),
            vec![ParameterId::mint("synthetic:test:id#length").expect("identity grammar")],
        ),
    ];
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#active").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    let mut annotations = AnnotationBuilder::new();

    super::attach_active_configuration_parameter_values(&mut ir, &mut annotations)
        .expect("valid exactness fields");

    assert_eq!(
        ir.model.configurations[0].parameter_values,
        BTreeMap::from([
            (
                ParameterId::mint("synthetic:test:id#angle").expect("identity grammar"),
                ParameterValue::Angle(Angle::new(std::f64::consts::FRAC_PI_2).unwrap())
            ),
            (
                ParameterId::mint("synthetic:test:id#length").expect("identity grammar"),
                ParameterValue::Length(Length::new(25.4).unwrap())
            ),
        ])
    );
}

#[test]
fn active_configuration_parameter_state_rejects_incomplete_sets_atomically() {
    let parameter = |id: &str, value, dependencies: Vec<ParameterId>| DesignParameter {
        id: ParameterId::mint(id).expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: id.into(),
        expression: id.into(),
        display: None,
        value,
        dependencies: (dependencies).try_into().unwrap(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let configuration = || DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#active").expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(cadmpeg_ir::features::DistinctMembers::default()),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    };
    let mut cases = [
        vec![parameter("synthetic:test:id#p1", None, Vec::new())],
        vec![parameter(
            "synthetic:test:id#p1",
            Some(ParameterValue::Real(
                cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
            )),
            vec![ParameterId::mint("synthetic:test:id#missing").expect("identity grammar")],
        )],
        vec![
            parameter(
                "synthetic:test:id#p1",
                Some(ParameterValue::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                )),
                Vec::new(),
            ),
            parameter(
                "synthetic:test:id#p1",
                Some(ParameterValue::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(2.0).unwrap(),
                )),
                Vec::new(),
            ),
        ],
        vec![
            parameter(
                "synthetic:test:id#p1",
                Some(ParameterValue::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
                )),
                Vec::new(),
            ),
            parameter(
                "synthetic:test:id#p2",
                Some(ParameterValue::Real(
                    cadmpeg_ir::scalar::FiniteReal::new(2.0).unwrap(),
                )),
                vec![ParameterId::mint("synthetic:test:id#p1").expect("identity grammar")],
            ),
        ],
    ];
    let mut annotations = AnnotationBuilder::new();
    for parameters in &mut cases {
        let mut ir = CadIr::empty();
        ir.model.parameters = std::mem::take(parameters);
        ir.model.configurations.push(configuration());

        super::attach_active_configuration_parameter_values(&mut ir, &mut annotations)
            .expect("valid exactness fields");

        assert!(ir.model.configurations[0].parameter_values.is_empty());
    }
}

#[test]
fn active_configuration_body_writers_close_false_suppression_through_dependencies() {
    let feature =
        |id: &str, dependencies: Vec<FeatureId>, outputs: Vec<BodyId>, suppressed| Feature {
            id: FeatureId::mint(id).expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed,
            dependencies: (dependencies).try_into().unwrap(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
                FeatureDefinition::Operation(FeatureOperation::TreeNode {
                    role: FeatureTreeNodeRole::History,
                    children: cadmpeg_ir::features::TreeChildren::default(),
                }),
                outputs,
            ),
            native_ref: None,
        };
    let configuration = |active, bodies| DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#configuration").expect("identity grammar"),
        ordinal: 0,
        active,
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    };
    let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.features = vec![
        feature("synthetic:test:id#dependency", Vec::new(), Vec::new(), None),
        feature(
            "synthetic:test:id#writer",
            vec![FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar")],
            vec![body.clone()],
            None,
        ),
        feature("synthetic:test:id#unrelated", Vec::new(), Vec::new(), None),
    ];
    for (ordinal, feature) in ir.model.features.iter_mut().enumerate() {
        feature.ordinal = ordinal as u64;
    }
    ir.model.configurations = vec![configuration(
        true,
        ConfigurationBodies::Resolved((vec![body]).try_into().unwrap()),
    )];
    let mut annotations = AnnotationBuilder::new();

    super::attach_active_configuration_feature_states(&mut ir, &mut annotations)
        .expect("valid exactness fields");

    assert_eq!(ir.model.features[0].suppressed, Some(false));
    assert_eq!(ir.model.features[1].suppressed, Some(false));
    assert_eq!(ir.model.features[2].suppressed, None);
    let states = &ir.model.configurations[0].feature_states;
    assert_eq!(
        states.keys().cloned().collect::<Vec<_>>(),
        [
            FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar"),
            FeatureId::mint("synthetic:test:id#writer").expect("identity grammar")
        ]
    );
    assert_eq!(
        states[&FeatureId::mint("synthetic:test:id#writer").expect("identity grammar")]
            .dependencies
            .as_slice(),
        [FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar")]
    );
    assert_eq!(
        states[&FeatureId::mint("synthetic:test:id#writer").expect("identity grammar")]
            .evaluation
            .outputs(),
        [BodyId::mint("test:model:entity#body").expect("identity grammar")]
    );
}

#[test]
fn current_body_writers_close_false_suppression_without_a_configuration() {
    let body = BodyId::mint("test:model:entity#body").expect("identity grammar");
    let feature = |id: &str, ordinal, dependencies: Vec<FeatureId>, outputs| Feature {
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: (dependencies).try_into().unwrap(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }),
            outputs,
        ),
        native_ref: None,
    };
    let mut ir = CadIr::empty();
    let mut body_record = cadmpeg_ir::examples::unit_cube().model.bodies.remove(0);
    body_record.id = body.clone();
    ir.model.bodies.push(body_record);
    ir.model.features = vec![
        feature("synthetic:test:id#dependency", 1, Vec::new(), Vec::new()),
        feature(
            "synthetic:test:id#writer",
            2,
            vec![FeatureId::mint("synthetic:test:id#dependency").expect("identity grammar")],
            vec![body],
        ),
        feature("synthetic:test:id#unrelated", 3, Vec::new(), Vec::new()),
    ];
    let mut annotations = AnnotationBuilder::new();

    super::attach_current_feature_states(&mut ir, &mut annotations)
        .expect("valid exactness fields");

    assert_eq!(ir.model.features[0].suppressed, Some(false));
    assert_eq!(ir.model.features[1].suppressed, Some(false));
    assert_eq!(ir.model.features[2].suppressed, None);

    ir.model.features[0].ordinal = 2;
    assert!(super::active_feature_closure(
        &ir,
        &[BodyId::mint("test:model:entity#body").expect("identity grammar")]
    )
    .is_err());
    ir.model.features[0].ordinal = 1;
    ir.model.features[2].id =
        FeatureId::mint("synthetic:test:id#writer").expect("identity grammar");
    assert!(super::active_feature_closure(
        &ir,
        &[BodyId::mint("test:model:entity#body").expect("identity grammar")]
    )
    .is_err());
    ir.model.features[2].id =
        FeatureId::mint("synthetic:test:id#unrelated").expect("identity grammar");
    ir.model.features[1].suppressed = Some(true);
    assert!(super::active_feature_closure(
        &ir,
        &[BodyId::mint("test:model:entity#body").expect("identity grammar")]
    )
    .is_err());
}

#[test]
fn active_configuration_feature_states_reject_incomplete_or_ambiguous_graphs_atomically() {
    let producer = |dependency: &str| Feature {
        id: FeatureId::mint("synthetic:test:id#writer").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: (vec![
            FeatureId::mint(format!("synthetic:test:id#{dependency}")).expect("identity grammar")
        ])
        .try_into()
        .unwrap(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: cadmpeg_ir::features::TreeChildren::default(),
            }),
            vec![BodyId::mint("test:model:entity#body").expect("identity grammar")],
        ),
        native_ref: None,
    };
    let configuration = |id: &str, active, bodies| DesignConfiguration {
        id: ConfigurationId::mint(id).expect("identity grammar"),
        ordinal: 0,
        active,
        source_index: Some(0),
        name: id.into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    };
    let mut missing_dependency = CadIr::empty();
    missing_dependency.model.features = vec![producer("missing")];
    missing_dependency.model.configurations = vec![configuration(
        "synthetic:test:id#active",
        true,
        ConfigurationBodies::Resolved(
            (vec![BodyId::mint("test:model:entity#body").expect("identity grammar")])
                .try_into()
                .unwrap(),
        ),
    )];
    let mut annotations = AnnotationBuilder::new();
    super::attach_active_configuration_feature_states(&mut missing_dependency, &mut annotations)
        .expect("valid exactness fields");
    assert_eq!(missing_dependency.model.features[0].suppressed, None);
    assert!(missing_dependency.model.configurations[0]
        .feature_states
        .is_empty());

    let mut unresolved_bodies = CadIr::empty();
    unresolved_bodies.model.features = vec![producer("writer")];
    unresolved_bodies.model.features[0].dependencies.clear();
    unresolved_bodies.model.configurations = vec![configuration(
        "synthetic:test:id#active",
        true,
        ConfigurationBodies::Unresolved,
    )];
    super::attach_active_configuration_feature_states(&mut unresolved_bodies, &mut annotations)
        .expect("valid exactness fields");
    assert_eq!(unresolved_bodies.model.features[0].suppressed, None);
    assert!(unresolved_bodies.model.configurations[0]
        .feature_states
        .is_empty());

    let mut contradicted = CadIr::empty();
    contradicted.model.features = vec![producer("writer")];
    contradicted.model.features[0].dependencies.clear();
    contradicted.model.features[0].suppressed = Some(true);
    contradicted.model.configurations = vec![configuration(
        "synthetic:test:id#active",
        true,
        ConfigurationBodies::Resolved(
            (vec![BodyId::mint("test:model:entity#body").expect("identity grammar")])
                .try_into()
                .unwrap(),
        ),
    )];
    super::attach_active_configuration_feature_states(&mut contradicted, &mut annotations)
        .expect("valid exactness fields");
    assert_eq!(contradicted.model.features[0].suppressed, Some(true));
    assert!(contradicted.model.configurations[0]
        .feature_states
        .is_empty());

    let mut ambiguous = CadIr::empty();
    ambiguous.model.features = vec![producer("writer")];
    ambiguous.model.features[0].dependencies.clear();
    ambiguous.model.configurations = vec![
        configuration(
            "synthetic:test:id#first",
            true,
            ConfigurationBodies::Resolved(
                (vec![BodyId::mint("test:model:entity#body").expect("identity grammar")])
                    .try_into()
                    .unwrap(),
            ),
        ),
        configuration(
            "synthetic:test:id#second",
            true,
            ConfigurationBodies::Resolved(
                (vec![BodyId::mint("test:model:entity#body").expect("identity grammar")])
                    .try_into()
                    .unwrap(),
            ),
        ),
    ];
    super::attach_active_configuration_feature_states(&mut ambiguous, &mut annotations)
        .expect("valid exactness fields");
    assert_eq!(ambiguous.model.features[0].suppressed, None);
    assert!(ambiguous
        .model
        .configurations
        .iter()
        .all(|configuration| configuration.feature_states.is_empty()));
}

#[test]
fn solved_sketch_points_require_unique_exact_ownership_atomically() {
    let label = crate::native::features::FeatureOperationLabel {
        id: "nx:feature-history:operation-label#section-7".to_string(),
        section_link: "section".to_string(),
        ordinal: 7,
        value: "SKETCH".to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 40,
    };
    let group = crate::native::features::FeatureSketchPointGroup {
        id: "point-group".to_string(),
        operation_label: label.id.clone(),
        name: "Point1".to_string(),
        points: vec!["payload-point".to_string()],
        coordinates: [12.5, -3.0],
    };
    let point_use = crate::native::features::FeatureSketchPointUse {
        id: "nx:feature-history:sketch-point-use#section-7-0".to_string(),
        operation_label: label.id.clone(),
        references: vec![crate::native::features::FeatureSketchPointUseReference {
            sketch_reference: "reference".to_string(),
            block_use: "block-use".to_string(),
            source_offset: 52,
        }],
        sketch_point_group: group.id.clone(),
        named_point: "named-point".to_string(),
    };
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let stream = annotations.stream("nx:container");
    let sketch = super::attach_sketch_graph(
        &mut ir,
        &label,
        &super::SketchSources {
            point_uses: &[&point_use],
            point_groups: std::slice::from_ref(&group),
            points: &[],
            payload_scalars: &[],
            fixed_points: &[],
            coordinate_pairs: &[],
        },
        &mut annotations,
        &stream,
    )
    .expect("one exact point use projects a sketch");
    assert_eq!(ir.model.sketches[0].id, sketch);
    assert!(matches!(
        *ir.model.sketch_entities[0].geometry.definition(),
        SketchGeometryDefinition::Point {
            position: Point2 { u: 12.5, v: -3.0 }
        }
    ));

    let mut rejected_ir = CadIr::empty();
    let mut rejected_annotations = AnnotationBuilder::new();
    let rejected_stream = rejected_annotations.stream("nx:container");
    assert!(super::attach_sketch_graph(
        &mut rejected_ir,
        &label,
        &super::SketchSources {
            point_uses: &[&point_use, &point_use],
            point_groups: &[group],
            points: &[],
            payload_scalars: &[],
            fixed_points: &[],
            coordinate_pairs: &[],
        },
        &mut rejected_annotations,
        &rejected_stream,
    )
    .is_none());
    assert!(rejected_ir.model.sketches.is_empty());
    assert!(rejected_ir.model.sketch_entities.is_empty());
}

#[test]
fn named_sketch_points_project_without_an_external_named_point() {
    let label = crate::native::features::FeatureOperationLabel {
        id: "nx:feature-history:operation-label#section-8".to_string(),
        section_link: "section".to_string(),
        ordinal: 8,
        value: "SKETCH".to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 40,
    };
    let point = crate::native::features::FeatureSketchPoint {
        id: "point".to_string(),
        operation_label: label.id.clone(),
        named_record: "named-record".to_string(),
        name: "Point1".to_string(),
        scalar_fields: ["scalar-1".to_string(), "scalar-2".to_string()],
        coordinates: [12.5, -3.0],
    };
    let group = crate::native::features::FeatureSketchPointGroup {
        id: "point-group".to_string(),
        operation_label: label.id.clone(),
        name: point.name.clone(),
        points: vec![point.id.clone()],
        coordinates: point.coordinates,
    };
    let scalar = |id: &str, ordinal: u32, value: f64, source_offset: u64| {
        crate::native::features::FeaturePayloadScalar {
            id: id.to_string(),
            operation_label: label.id.clone(),
            payload: crate::native::features::FeatureScalarPayload::Construction {
                construction_payload: "payload".to_string(),
            },
            ordinal,
            field_code: 100,
            scalar: {
                let mut raw = value.to_be_bytes();
                raw[0] -= 0x10;
                crate::om::scalar::ShiftedBinary64::try_from(raw).unwrap()
            },
            payload_offset: ordinal as u64,
            source_offset,
        }
    };
    let scalars = [
        scalar("scalar-1", 0, 12.5, 51),
        scalar("scalar-2", 1, -3.0, 59),
    ];
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let stream = annotations.stream("nx:container");
    let sketch = super::attach_sketch_graph(
        &mut ir,
        &label,
        &super::SketchSources {
            point_uses: &[],
            point_groups: std::slice::from_ref(&group),
            points: std::slice::from_ref(&point),
            payload_scalars: &scalars,
            fixed_points: &[],
            coordinate_pairs: &[],
        },
        &mut annotations,
        &stream,
    )
    .expect("a complete named payload point projects a sketch");
    assert_eq!(ir.model.sketches[0].id, sketch);
    assert_eq!(ir.model.sketch_entities.len(), 1);
    assert_eq!(
        ir.model.sketch_entities[0].native_ref.as_deref(),
        Some("point-group")
    );
    assert!(matches!(
        *ir.model.sketch_entities[0].geometry.definition(),
        SketchGeometryDefinition::Point {
            position: Point2 { u: 12.5, v: -3.0 }
        }
    ));
}

#[test]
fn nx_native_feature_parameters_require_unique_resolved_names() {
    let expression = |id: &str, name: &str, text: &str| crate::native::om::Expression {
        id: id.to_string(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.to_string()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: text.to_string(),
        value: None,
        source_entry: "entry".to_string(),
        source_table: cadmpeg_ir::NonEmptyString::new("nx:test:expression-table#table").unwrap(),
        source_offset: 0,
    };
    let parameter_use = |id: &str, expression: &str| crate::native::features::FeatureParameterUse {
        id: id.to_string(),
        operation_label: "operation".to_string(),
        expression: expression.to_string(),
        bindings: vec![crate::native::features::FeatureParameterUseBinding {
            binding: format!("binding-{id}"),
            source_offset: 0,
        }],
    };
    let expressions = vec![
        expression("expression-a", "p1_length", "p2_length * 2"),
        expression("expression-b", "p2_length", "12.5"),
    ];
    let uses = [
        parameter_use("use-a", "expression-a"),
        parameter_use("use-b", "expression-b"),
    ];
    let use_refs = uses.iter().collect::<Vec<_>>();
    let parameters = super::native_feature_parameters(&use_refs, &expressions);
    assert_eq!(
        parameters,
        std::collections::BTreeMap::from([
            ("p1_length".to_string(), "p2_length * 2".to_string()),
            ("p2_length".to_string(), "12.5".to_string()),
        ])
    );
    assert_eq!(
        super::non_boolean_feature_definition_with_parameters(
            "UNKNOWN OPERATION",
            &[],
            None,
            None,
            super::HoleProjection::default(),
            parameters,
        )
        .unwrap(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native {
                kind: "UNKNOWN OPERATION".into(),
                parameters: std::collections::BTreeMap::from([
                    ("p1_length".to_string(), "p2_length * 2".to_string()),
                    ("p2_length".to_string(), "12.5".to_string()),
                ]),
            }
        )
    );
    assert!(matches!(
        super::non_boolean_feature_definition_with_parameters(
            "DELETE",
            &[],
            None,
            None,
            super::HoleProjection::default(),
            std::collections::BTreeMap::default(),
        ).unwrap(),
        cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Native { kind, .. }) if kind.as_str() == "DELETE"
    ));
    assert!(matches!(
        super::non_boolean_feature_definition_with_parameters(
            "THRU_CURVE",
            &[],
            None,
            None,
            super::HoleProjection::default(),
            std::collections::BTreeMap::new(),
        )
        .unwrap(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Unresolved {
                family: cadmpeg_ir::features::UnresolvedFamily::Loft
            }
        )
    ));
    assert!(matches!(
        super::non_boolean_feature_definition_with_parameters(
            "SWP104",
            &[],
            None,
            None,
            super::HoleProjection::default(),
            std::collections::BTreeMap::new(),
        ).unwrap(), cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sweep {
            shape,
            path: None,

            ..
        }) if matches!((shape.section(), shape.mode(),), (cadmpeg_ir::features::SweepSection::Unresolved(_), cadmpeg_ir::features::SweepMode::Unresolved {},))));
    let duplicate_expressions = vec![
        expression("expression-a", "p1_length", "1"),
        expression("expression-b", "p1_length", "2"),
    ];
    assert!(super::native_feature_parameters(&use_refs, &duplicate_expressions).is_empty());
    let unresolved = [parameter_use("use-c", "missing")];
    assert!(
        super::native_feature_parameters(&unresolved.iter().collect::<Vec<_>>(), &expressions,)
            .is_empty()
    );
}

#[test]
fn nx_intersection_labels_project_without_fabricating_construction_fields() {
    for operation in ["ASSOCIATIVE_INTERSECTION", "Intersection Curve"] {
        assert!(matches!(
            super::non_boolean_feature_definition_with_parameters(
                operation,
                &[],
                None,
                None,
                super::HoleProjection::default(),
                std::collections::BTreeMap::default(),
            ).unwrap(), cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::SectionShape {
                operands,

                approximate: None,
            }) if matches!((operands.first(), operands.second(),), (cadmpeg_ir::features::BodySelection::Unresolved, cadmpeg_ir::features::BodySelection::Unresolved,))));
    }
}

#[test]
fn nx_multi_instance_output_projects_as_an_unresolved_pattern() {
    assert!(
        matches!(&(super::non_boolean_feature_definition_with_parameters(
                "Multi Instance Output",
                &[],
                None,
                None,
                super::HoleProjection::default(),
                std::collections::BTreeMap::default(),
            ).unwrap()),
            cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern {
                seeds,
                pattern: admitted_pattern,
            }) if matches!(admitted_pattern.definition(), cadmpeg_ir::features::PatternTransform::Unresolved { form: None } if seeds.is_empty())
        )
    );
}

#[test]
fn boolean_target_is_an_independent_intermediate_result_writer() {
    use crate::native::features::{
        FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation,
    };

    let boolean = FeatureBooleanOperation {
        id: "nx:test:boolean#0".into(),
        operation_label: "nx:test:operation#0".into(),
        kind: FeatureBooleanKind::Unite,
        target: crate::test_support::native_references::boolean_reference(7, 1),
        tools: vec![crate::test_support::native_references::boolean_reference(
            8, 2,
        )],
        source_offset: 0,
    };
    assert_eq!(
        super::native_result_body_identity(None, Some(&boolean)),
        Some((
            "nx:test:boolean#0:target".into(),
            "nx:test:boolean#0".into(),
        ))
    );

    let primary = FeatureBodyReference {
        ordinal: None,
        id: "nx:test:primary#0".into(),
        operation_label: boolean.operation_label.clone(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(7, &[7]).unwrap(),
        source_offset: 3,
    };
    assert_eq!(
        super::native_result_body_identity(Some(&primary), Some(&boolean)),
        Some(("nx:test:primary#0".into(), "nx:test:primary#0".into(),))
    );
}

#[test]
fn boolean_target_output_requires_one_resolved_segment_body() {
    use cadmpeg_ir::features::{BodySelection, BooleanKind, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::BodyId;

    let body = BodyId::mint("nx:s0:body#0").expect("identity grammar");
    let definition = FeatureDefinition::Operation(FeatureOperation::Combine {
        operands: cadmpeg_ir::features::CombineOperands::new(
            BodySelection::Resolved {
                bodies: vec![body.clone()],
                native: "target".into(),
            },
            BodySelection::Unresolved,
        )
        .unwrap(),

        op: BooleanKind::Join,
        keep_tools: false,
    });
    assert_eq!(super::boolean_target_output(Some(&definition)), Some(body));

    let ambiguous = FeatureDefinition::Operation(FeatureOperation::Combine {
        operands: cadmpeg_ir::features::CombineOperands::new(
            BodySelection::Unresolved,
            BodySelection::Unresolved,
        )
        .unwrap(),

        op: BooleanKind::Join,
        keep_tools: false,
    });
    assert!(super::boolean_target_output(Some(&ambiguous)).is_none());
}

#[test]
fn topology_inferred_hole_axis_is_not_an_authored_direction() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, HolePlacement};
    use cadmpeg_ir::math::{Point3, Vector3};

    for kind in ["SIMPLE HOLE", "HOLE PACKAGE"] {
        assert!(matches!(
            super::non_boolean_feature_definition_with_parameters(
                kind,
                &[],
                None,
                None,
                super::HoleProjection {
                    placements: vec![HolePlacement::Axis {
                        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
                        axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
                    }],
                    ..super::HoleProjection::default()
                },
                std::collections::BTreeMap::new(),
            ).unwrap(),
            FeatureDefinition::Operation(FeatureOperation::Hole {
                placements,
                ..
            }) if placements.as_deref() == Some(&[HolePlacement::Axis {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
                axis: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            }][..])
        ));
    }
}

#[test]
fn complete_extrude_profile_projects_without_guessing_scalar_roles() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureOperation,
        LinearTermination, PlanarProfileRef, ProfileRef,
    };

    assert_eq!(
        super::extrude_feature_definition(
            Some("nx:profile#1"),
            None,
            BooleanOp::NewBody,
            &[cadmpeg_ir::topology::BodyKind::Solid],
        ),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Native("nx:profile#1".to_string())),
            direction: cadmpeg_ir::features::ExtrudeDirection::Unresolved {},
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Unresolved {},
                    draft: None,
                },
            },
            op: BooleanOp::NewBody,
            start: cadmpeg_ir::features::ExtrudeStart::Unresolved {},
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        })
    );
    assert!(matches!(
        super::extrude_feature_definition(
            None,
            None,
            BooleanOp::Unresolved,
            &[cadmpeg_ir::topology::BodyKind::Sheet],
        ),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            solid: Some(false),
            ..
        })
    ));
    assert!(matches!(
        super::extrude_feature_definition(
            Some("nx:profile#1"),
            Some("nx:profile#2"),
            BooleanOp::Unresolved,
            &[
                cadmpeg_ir::topology::BodyKind::Solid,
                cadmpeg_ir::topology::BodyKind::Sheet,
            ],
        ),
        FeatureDefinition::Operation(FeatureOperation::Extrude {
            profile: ProfileRef::Planar(PlanarProfileRef::Unresolved(_)),
            solid: None,
            ..
        })
    ));
}

#[test]
fn extrusion_is_new_body_only_for_one_first_written_surface_or_solid_output() {
    use cadmpeg_ir::features::BooleanOp;
    use cadmpeg_ir::topology::BodyKind;

    let history = super::BodyWriterHistory::default();
    assert_eq!(
        super::extrude_boolean_op(&history, Some(7), None, &[BodyKind::Solid]),
        BooleanOp::NewBody
    );
    assert_eq!(
        super::extrude_boolean_op(
            &super::BodyWriterHistory::default(),
            None,
            None,
            &[BodyKind::Solid],
        ),
        BooleanOp::Unresolved
    );
    assert_eq!(
        super::extrude_boolean_op(&history, Some(7), None, &[BodyKind::Sheet]),
        BooleanOp::NewBody
    );
    assert_eq!(
        super::extrude_boolean_op(&history, Some(7), None, &[BodyKind::Wire]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        super::extrude_boolean_op(&history, Some(7), None, &[BodyKind::General]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        super::extrude_boolean_op(&history, Some(7), None, &[BodyKind::Solid, BodyKind::Solid],),
        BooleanOp::Unresolved
    );
    assert_eq!(
        super::extrude_boolean_op(&history, Some(7), None, &[]),
        BooleanOp::Unresolved
    );

    let prior =
        super::FeatureId::mint("synthetic:test:id#prior-offset-writer").expect("identity grammar");
    let offset_body = "store:block#7";
    let mut offset_history = super::BodyWriterHistory::default();
    offset_history.record_writer(None, Some(offset_body), &[], &prior);
    assert_eq!(
        super::extrude_boolean_op(&offset_history, None, Some(offset_body), &[BodyKind::Solid]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        super::extrude_boolean_op(
            &offset_history,
            None,
            Some("store:block#8"),
            &[BodyKind::Solid],
        ),
        BooleanOp::NewBody
    );
}

#[test]
fn nx_block_dimension_parameters_name_the_block_as_consumer() {
    let expression = |key: u32| crate::native::om::Expression {
        id: format!("nx:test:expression#{key}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(format!("p{key}")),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: key.to_string(),
        value: Some(
            crate::native::om::finite_value::FiniteValue::try_from(f64::from(key)).unwrap(),
        ),
        source_entry: "part".into(),
        source_table: cadmpeg_ir::NonEmptyString::new("nx:test:expression-table#table").unwrap(),
        source_offset: u64::from(key),
    };
    let expressions = [expression(20), expression(21), expression(22)];
    let dimensions = crate::native::features::FeatureBlockDimensions {
        id: "dimensions".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        construction: "construction".into(),
        anchor_bindings: vec!["binding".into()],
        dimensions: std::array::from_fn(|slot| crate::native::features::FeatureBlockDimension {
            declaration: ["d20", "d21", "d22"][slot].into(),
            expression: expressions[slot].id.clone(),
            value: [20.0, 21.0, 22.0][slot],
        }),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    super::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations)
        .expect("valid exactness fields");
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect();
    let parameter_references = dimensions
        .dimensions
        .iter()
        .filter_map(|dimension| super::expression_parameter_id(&dimension.expression))
        .collect::<Vec<_>>();
    assert_eq!(
        super::parameter_owner_dependencies(&parameter_owners, &parameter_references),
        [ir.model.features[0].id.clone()]
    );
    assert_eq!(
        ir.model.features[0].source_content.as_slice(),
        ir.model
            .parameters
            .iter()
            .map(|parameter| {
                cadmpeg_ir::features::FeatureSourceContent::Parameter(parameter.id.clone())
            })
            .collect::<Vec<_>>()
    );
    super::attach_block_dimension_parameter_consumers(&mut ir, &[dimensions], &mut annotations)
        .expect("valid exactness fields");
    assert_eq!(ir.model.parameters.len(), 3);
    for (ordinal, parameter) in ir.model.parameters.iter().enumerate() {
        assert_eq!(
            parameter.properties[&format!("block_dimension.{ordinal}")],
            "dimensions"
        );
        assert_eq!(
            parameter.properties["consumer.0"],
            "nx:feature-history:feature#1-4"
        );
    }
}

#[test]
fn nx_inch_expression_values_are_attached_in_millimeters() {
    let expression =
        |key: u32, name: &str, formula: &str, value: Option<f64>| crate::native::om::Expression {
            id: format!("nx:test:expression#{key}"),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: crate::native::om::ExpressionUnit::Inch,
            expression: formula.into(),
            value: value.map(|value| {
                crate::native::om::finite_value::FiniteValue::try_from(value).unwrap()
            }),
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: cadmpeg_ir::NonEmptyString::new("nx:test:expression-table#table")
                .unwrap(),
            source_offset: u64::from(key),
        };
    let expressions = [
        expression(1, "p1", "2", Some(2.0)),
        expression(2, "p2", "p1 * 3", Some(6.0)),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    super::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations)
        .expect("valid exactness fields");

    assert_eq!(
        ir.model.parameters[0].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(2.0 * 25.4).unwrap()
        ))
    );
    assert_eq!(
        ir.model.parameters[1].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(6.0 * 25.4).unwrap()
        ))
    );
    assert_eq!(
        ir.model.parameters[0]
            .properties
            .get("unit")
            .map(String::as_str),
        Some("inch")
    );
    assert!(incomplete_expression_parameters(&ir).is_empty());
}

#[test]
fn nx_native_expression_units_remain_outside_neutral_values() {
    let expression = crate::native::om::Expression {
        id: "nx:test:expression#native".into(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new("p1".to_string()),
        unit: crate::native::om::ExpressionUnit::Native("custom/unit".into()),
        expression: "4".into(),
        value: Some(crate::native::om::finite_value::FiniteValue::try_from(4.0).unwrap()),
        source_entry: "part".into(),
        source_table: cadmpeg_ir::NonEmptyString::new("nx:test:expression-table#table").unwrap(),
        source_offset: 1,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    super::attach_expression_parameters(&mut ir, &[expression], &[], &[], &mut annotations)
        .expect("valid exactness fields");

    assert_eq!(ir.model.parameters[0].value, None);
    assert_eq!(
        ir.model.parameters[0]
            .properties
            .get("unit")
            .map(String::as_str),
        Some("custom/unit")
    );
    assert_eq!(
        incomplete_expression_parameters(&ir),
        [ir.model.parameters[0].id.clone()].into()
    );
}

mod colors;

mod body_selection;
