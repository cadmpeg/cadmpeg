// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]

use std::io::{Cursor, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::LossCategory;
use cadmpeg_ir::Exactness;

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::test_support::*;
use crate::NxCodec;

use super::*;

#[test]
fn rm_face_colors_require_unique_palette_topology_and_stream_joins() {
    let definition = crate::native::om::PartColorDefinition {
        id: "nx:test:color#201".into(),
        color_table: "nx:test:table#0".into(),
        color_index: 201,
        name: "Iron Gray".into(),
        rgb: [0.25, 0.5, 0.75],
        raw_color_index: vec![0x80, 200],
        raw_components: [vec![1], vec![1], vec![1]],
        source_offset: 10,
        component_source_offsets: [11, 12, 13],
    };
    let assignment = crate::native::om::RmDisplayColorAssignment {
        id: "nx:test:assignment#0".into(),
        ordinal: 0,
        encoding: crate::native::om::RmDisplayColorAssignmentEncoding::Linked {
            object_index: 42,
            raw_object_index: vec![42],
            object_index_source_offset: 22,
            discriminator: 0x16,
            target_index: 7,
            raw_target_index: vec![7],
            target_index_source_offset: 23,
            indices: [1, 2, 3],
            raw_indices: [vec![1], vec![2], vec![3]],
            index_source_offsets: [24, 25, 26],
            flag: 3,
            mode: 4,
        },
        target_object_id: Some("nx:test:object-id#7".into()),
        color_index: 201,
        color_definition: definition.id.clone(),
        raw_color_index: vec![0x80, 201],
        source_entry: "/Root/FastLoad/RMFastLoad".into(),
        source_offset: 20,
        row_source_offset: 21,
    };
    let record = crate::native::parasolid::ParasolidDeltasRecord {
        id: "nx:test:deltas#0".into(),
        stream_ordinal: 1,
        family: "FACE".into(),
        kind: 14,
        xmt: 99,
        node_id: Some(42),
        references: Vec::new(),
        position: None,
        byte_len: 1,
        inflated_offset: 0,
    };
    let face_ids = BTreeSet::from(["nx:s0:face#99".to_string()]);
    let pairs = BTreeMap::from([(0, vec![1])]);
    assert_eq!(
        resolve_rm_face_colors(
            &face_ids,
            std::slice::from_ref(&assignment),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&record),
            &pairs,
        ),
        vec![(
            "nx:s0:face#99".into(),
            Color {
                r: 0.25,
                g: 0.5,
                b: 0.75,
                a: 1.0,
            },
        )]
    );

    assert_eq!(
        resolve_rm_face_color_bindings(
            &face_ids,
            std::slice::from_ref(&assignment),
            std::slice::from_ref(&definition),
            std::slice::from_ref(&record),
            &pairs,
        ),
        vec![RmFaceColorBinding {
            face_id: "nx:s0:face#99".into(),
            color_definition: definition.id.clone(),
            source_offset: 20,
        }]
    );

    let mut target_assignment = assignment.clone();
    target_assignment.encoding = crate::native::om::RmDisplayColorAssignmentEncoding::Target {
        target_index: 7,
        raw_target_index: vec![7],
        target_index_source_offset: 23,
        indices: [1, 2, 3],
        raw_indices: [vec![1], vec![2], vec![3]],
        index_source_offsets: [24, 25, 26],
        mode: 4,
    };
    assert_eq!(
        resolve_rm_face_colors(
            &face_ids,
            &[assignment.clone(), target_assignment],
            std::slice::from_ref(&definition),
            std::slice::from_ref(&record),
            &pairs,
        ),
        vec![(
            "nx:s0:face#99".into(),
            Color {
                r: 0.25,
                g: 0.5,
                b: 0.75,
                a: 1.0,
            },
        )]
    );

    let mut conflicting = assignment;
    conflicting.color_definition = "nx:test:color#other".into();
    assert!(
        resolve_rm_face_colors(&face_ids, &[conflicting], &[definition], &[record], &pairs,)
            .is_empty()
    );
}

#[test]
fn rm_source_color_bindings_require_one_palette_per_source_identity() {
    let assignment = |id: &str, source_id: Option<&str>, color_definition: &str, offset| {
        crate::native::om::RmDisplayColorAssignment {
            id: id.into(),
            ordinal: 0,
            encoding: crate::native::om::RmDisplayColorAssignmentEncoding::Target {
                target_index: 7,
                raw_target_index: vec![7],
                target_index_source_offset: offset,
                indices: [1, 2, 3],
                raw_indices: [vec![1], vec![2], vec![3]],
                index_source_offsets: [offset + 1, offset + 2, offset + 3],
                mode: 4,
            },
            target_object_id: source_id.map(str::to_owned),
            color_index: 201,
            color_definition: color_definition.into(),
            raw_color_index: vec![0x80, 201],
            source_entry: "/Root/FastLoad/RMFastLoad".into(),
            source_offset: offset,
            row_source_offset: offset,
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
    use crate::native::features::{
        FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleTemplate, SimpleHoleEndTreatment,
        SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };

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
        operation_labels: vec!["operation#newer".into(), "operation#older".into()],
        scalar_lanes: vec!["lane-newer".into(), "lane-older".into()],
        block_references: vec!["blocks-newer".into(), "blocks-older".into()],
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
    let duplicate_group = FeatureSimpleHoleConstructionGroup {
        id: "duplicate-group".into(),
        first_data_blocks: ["a".into(), "b".into()],
        second_data_blocks: ["c".into(), "d".into()],
        operation_labels: vec![
            "operation#older".into(),
            "operation#newer".into(),
            "operation#older".into(),
        ],
        scalar_lanes: vec!["lane-a".into(), "lane-b".into()],
        block_references: vec!["refs-a".into(), "refs-b".into()],
    };
    assert!(
        simple_hole_operations(&templates, &[duplicate_group], &operation_positions,).is_none()
    );
}

#[test]
fn exact_hole_package_owns_common_internal_simple_holes() {
    use crate::native::features::{
        FeatureHolePackageConstructionGroupUse, FeatureSimpleHoleConstructionGroup,
        FeatureSimpleHoleTemplate, SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily,
        SimpleHoleForm,
    };
    use cadmpeg_ir::features::{Angle, HoleKind, Length};
    use cadmpeg_ir::ids::BodyId;

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
        operation_labels: operations.to_vec(),
        scalar_lanes: vec!["lane-a".into(), "lane-b".into()],
        block_references: vec!["blocks-a".into(), "blocks-b".into()],
    };
    let use_ = FeatureHolePackageConstructionGroupUse {
        id: "use".into(),
        operation_label: "package".into(),
        construction_group_lane: "package-lane".into(),
        simple_hole_construction_group: group.id.clone(),
        source_offset: 0,
    };
    let body = BodyId("body".into());
    let outputs = operations
        .iter()
        .map(|operation| (operation.clone(), vec![body.clone()]))
        .collect();
    let diameters = operations
        .iter()
        .map(|operation| (operation.clone(), Length(5.1)))
        .collect();
    let chamfer = HoleKind::Chamfer {
        diameter: Length(7.1),
        angle: Angle(std::f64::consts::FRAC_PI_2),
    };
    let chamfers = operations
        .iter()
        .map(|operation| (operation.clone(), chamfer))
        .collect();

    let projection = super::hole_package_projection(
        &cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default()),
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
    assert_eq!(projection.diameters["package"], Length(5.1));
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
        &cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default()),
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
    assert_eq!(projection.diameters["package"], Length(5.1));
    assert!(!projection.chamfers.contains_key("package"));

    let mut mixed_templates = untreated_templates.clone();
    mixed_templates[0].start_treatment = SimpleHoleEndTreatment::Chamfer;
    let projection = super::hole_package_projection(
        &cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default()),
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
    mismatched_outputs.insert("simple-b".into(), vec![BodyId("other-body".into())]);
    let projection = super::hole_package_projection(
        &cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default()),
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
        id: ParameterId(id.into()),
        owner: None,
        ordinal,
        name: id.into(),
        expression: id.into(),
        display: None,
        value,
        dependencies,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.parameters = vec![
        parameter(
            "length",
            0,
            Some(ParameterValue::Length(Length(25.4))),
            Vec::new(),
        ),
        parameter(
            "angle",
            1,
            Some(ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2))),
            vec![ParameterId("length".into())],
        ),
    ];
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("active".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        suppressed_features: Vec::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    let mut annotations = AnnotationBuilder::new();

    super::attach_active_configuration_parameter_values(&mut ir, &mut annotations);

    assert_eq!(
        ir.model.configurations[0].parameter_values,
        BTreeMap::from([
            (
                ParameterId("angle".into()),
                ParameterValue::Angle(Angle(std::f64::consts::FRAC_PI_2))
            ),
            (
                ParameterId("length".into()),
                ParameterValue::Length(Length(25.4))
            ),
        ])
    );
}

#[test]
fn active_configuration_parameter_state_rejects_incomplete_sets_atomically() {
    let parameter = |id: &str, value, dependencies: Vec<ParameterId>| DesignParameter {
        id: ParameterId(id.into()),
        owner: None,
        ordinal: 0,
        name: id.into(),
        expression: id.into(),
        display: None,
        value,
        dependencies,
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    };
    let configuration = || DesignConfiguration {
        id: ConfigurationId("active".into()),
        ordinal: 0,
        active: true.into(),
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        suppressed_features: Vec::new(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    };
    let mut cases = [
        vec![parameter("p1", None, Vec::new())],
        vec![parameter(
            "p1",
            Some(ParameterValue::Real(1.0)),
            vec![ParameterId("missing".into())],
        )],
        vec![
            parameter("p1", Some(ParameterValue::Real(1.0)), Vec::new()),
            parameter("p1", Some(ParameterValue::Real(2.0)), Vec::new()),
        ],
        vec![
            parameter("p1", Some(ParameterValue::Real(1.0)), Vec::new()),
            parameter(
                "p2",
                Some(ParameterValue::Real(2.0)),
                vec![ParameterId("p1".into())],
            ),
        ],
    ];
    let mut annotations = AnnotationBuilder::new();
    for parameters in &mut cases {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.parameters = std::mem::take(parameters);
        ir.model.configurations.push(configuration());

        super::attach_active_configuration_parameter_values(&mut ir, &mut annotations);

        assert!(ir.model.configurations[0].parameter_values.is_empty());
    }
}

#[test]
fn active_configuration_body_writers_close_false_suppression_through_dependencies() {
    let feature =
        |id: &str, dependencies: Vec<FeatureId>, outputs: Vec<BodyId>, suppressed| Feature {
            id: FeatureId(id.into()),
            ordinal: 0,
            name: None,
            suppressed,
            parent: None,
            dependencies,
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        };
    let configuration = |active, bodies| DesignConfiguration {
        id: ConfigurationId("configuration".into()),
        ordinal: 0,
        active,
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        suppressed_features: Vec::new(),
        bodies,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    };
    let body = BodyId("body".into());
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features = vec![
        feature("dependency", Vec::new(), Vec::new(), None),
        feature(
            "writer",
            vec![FeatureId("dependency".into())],
            vec![body.clone()],
            None,
        ),
        feature("unrelated", Vec::new(), Vec::new(), None),
    ];
    for (ordinal, feature) in ir.model.features.iter_mut().enumerate() {
        feature.ordinal = ordinal as u64;
    }
    ir.model.configurations = vec![configuration(
        true.into(),
        ConfigurationBodies::Resolved(vec![body]),
    )];
    let mut annotations = AnnotationBuilder::new();

    super::attach_active_configuration_feature_states(&mut ir, &mut annotations);

    assert_eq!(ir.model.features[0].suppressed, Some(false));
    assert_eq!(ir.model.features[1].suppressed, Some(false));
    assert_eq!(ir.model.features[2].suppressed, None);
    let states = &ir.model.configurations[0].feature_states;
    assert_eq!(
        states.keys().cloned().collect::<Vec<_>>(),
        [FeatureId("dependency".into()), FeatureId("writer".into())]
    );
    assert_eq!(
        states[&FeatureId("writer".into())].dependencies,
        [FeatureId("dependency".into())]
    );
    assert_eq!(
        states[&FeatureId("writer".into())].outputs,
        [BodyId("body".into())]
    );
}

#[test]
fn current_body_writers_close_false_suppression_without_a_configuration() {
    let body = BodyId("body".into());
    let feature = |id: &str, ordinal, dependencies, outputs| Feature {
        id: FeatureId(id.into()),
        ordinal,
        name: None,
        suppressed: None,
        parent: None,
        dependencies,
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs,
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    };
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut body_record = cadmpeg_ir::examples::unit_cube().model.bodies.remove(0);
    body_record.id = body.clone();
    ir.model.bodies.push(body_record);
    ir.model.features = vec![
        feature("dependency", 1, Vec::new(), Vec::new()),
        feature(
            "writer",
            2,
            vec![FeatureId("dependency".into())],
            vec![body],
        ),
        feature("unrelated", 3, Vec::new(), Vec::new()),
    ];
    let mut annotations = AnnotationBuilder::new();

    super::attach_current_feature_states(&mut ir, &mut annotations);

    assert_eq!(ir.model.features[0].suppressed, Some(false));
    assert_eq!(ir.model.features[1].suppressed, Some(false));
    assert_eq!(ir.model.features[2].suppressed, None);

    ir.model.features[0].ordinal = 2;
    assert!(super::active_feature_closure(&ir, &[BodyId("body".into())]).is_none());
    ir.model.features[0].ordinal = 1;
    ir.model.features[2].id = FeatureId("writer".into());
    assert!(super::active_feature_closure(&ir, &[BodyId("body".into())]).is_none());
    ir.model.features[2].id = FeatureId("unrelated".into());
    ir.model.features[1].suppressed = Some(true);
    assert!(super::active_feature_closure(&ir, &[BodyId("body".into())]).is_none());
}

#[test]
fn active_configuration_feature_states_reject_incomplete_or_ambiguous_graphs_atomically() {
    let producer = |dependency: &str| Feature {
        id: FeatureId("writer".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: vec![FeatureId(dependency.into())],
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![BodyId("body".into())],
        definition: FeatureDefinition::TreeNode {
            role: FeatureTreeNodeRole::History,
            children: Vec::new(),
            active_child: None,
        },
        native_ref: None,
    };
    let configuration = |id: &str, active, bodies| DesignConfiguration {
        id: ConfigurationId(id.into()),
        ordinal: 0,
        active,
        source_index: Some(0),
        name: id.into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        suppressed_features: Vec::new(),
        bodies,
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    };
    let mut missing_dependency = CadIr::empty(cadmpeg_ir::units::Units::default());
    missing_dependency.model.features = vec![producer("missing")];
    missing_dependency.model.configurations = vec![configuration(
        "active",
        true.into(),
        ConfigurationBodies::Resolved(vec![BodyId("body".into())]),
    )];
    let mut annotations = AnnotationBuilder::new();
    super::attach_active_configuration_feature_states(&mut missing_dependency, &mut annotations);
    assert_eq!(missing_dependency.model.features[0].suppressed, None);
    assert!(missing_dependency.model.configurations[0]
        .feature_states
        .is_empty());

    let mut unresolved_bodies = CadIr::empty(cadmpeg_ir::units::Units::default());
    unresolved_bodies.model.features = vec![producer("writer")];
    unresolved_bodies.model.features[0].dependencies.clear();
    unresolved_bodies.model.configurations = vec![configuration(
        "active",
        true.into(),
        ConfigurationBodies::Unresolved,
    )];
    super::attach_active_configuration_feature_states(&mut unresolved_bodies, &mut annotations);
    assert_eq!(unresolved_bodies.model.features[0].suppressed, None);
    assert!(unresolved_bodies.model.configurations[0]
        .feature_states
        .is_empty());

    let mut contradicted = CadIr::empty(cadmpeg_ir::units::Units::default());
    contradicted.model.features = vec![producer("writer")];
    contradicted.model.features[0].dependencies.clear();
    contradicted.model.features[0].suppressed = Some(true);
    contradicted.model.configurations = vec![configuration(
        "active",
        true.into(),
        ConfigurationBodies::Resolved(vec![BodyId("body".into())]),
    )];
    super::attach_active_configuration_feature_states(&mut contradicted, &mut annotations);
    assert_eq!(contradicted.model.features[0].suppressed, Some(true));
    assert!(contradicted.model.configurations[0]
        .feature_states
        .is_empty());

    let mut ambiguous = CadIr::empty(cadmpeg_ir::units::Units::default());
    ambiguous.model.features = vec![producer("writer")];
    ambiguous.model.features[0].dependencies.clear();
    ambiguous.model.configurations = vec![
        configuration(
            "first",
            true.into(),
            ConfigurationBodies::Resolved(vec![BodyId("body".into())]),
        ),
        configuration(
            "second",
            true.into(),
            ConfigurationBodies::Resolved(vec![BodyId("body".into())]),
        ),
    ];
    super::attach_active_configuration_feature_states(&mut ambiguous, &mut annotations);
    assert_eq!(ambiguous.model.features[0].suppressed, None);
    assert!(ambiguous
        .model
        .configurations
        .iter()
        .all(|configuration| configuration.feature_states.is_empty()));
}

#[test]
fn operation_source_properties_require_unique_owned_structures() {
    let record = crate::native::features::FeatureOperationRecord {
        id: "record".into(),
        operation_label: "operation".into(),
        ordinal: 3,
        byte_len: 20,
        sha256: "record-hash".into(),
        payload_byte_len: 10,
        payload_sha256: "payload-hash".into(),
        payload_source_offset: 110,
        source_offset: 100,
    };
    let common = crate::native::features::FeatureOperationCommonFrame {
        id: "common".into(),
        operation_record: record.id.clone(),
        ordinal: 0,
        indices: [0, 351, 171],
        raw_indices: [vec![0], vec![0x81, 0x5f], vec![0x80, 0xab]],
        marker: [1, 3, 2],
        state: [1, 2, 1, 1, 1, 0, 0, 0],
        legacy_inactive_modules: Some(true),
        modifies_parasolid_data: Some(true),
        split_tracking_data: [0, 0],
        group_count: 0,
        local_ordinal: 41,
        raw_local_ordinal: vec![0x29],
        object_index: Some(65),
        raw_object_index: vec![0x41],
        data_block: None,
        byte_len: 20,
        source_offset: 101,
        index_source_offsets: [101, 102, 104],
        state_source_offset: 109,
        local_ordinal_source_offset: 117,
        object_index_source_offset: 119,
    };
    let frame = crate::native::features::FeatureOperationTerminalFrame {
        id: "frame".into(),
        operation_record: record.id.clone(),
        immediate_common_frame: Some(common.id.clone()),
        local_ordinal: 41,
        raw_local_ordinal: vec![0x29],
        object_index: Some(65),
        raw_object_index: vec![0x41],
        data_block: None,
        source_offset: 117,
        object_index_source_offset: 119,
    };
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            std::slice::from_ref(&common),
            std::slice::from_ref(&frame),
        ),
        BTreeMap::from([
            ("operation_common_frame.0".into(), "common".into()),
            ("operation_record".into(), "record".into()),
            ("operation_terminal_frame".into(), "frame".into()),
        ])
    );
    assert!(super::operation_source_properties("missing", &[], &[], &[]).is_empty());
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            &[],
            &[],
        ),
        BTreeMap::from([("operation_record".into(), "record".into())])
    );
    let mut noncontiguous_common = common.clone();
    noncontiguous_common.ordinal = 1;
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            std::slice::from_ref(&noncontiguous_common),
            std::slice::from_ref(&frame),
        ),
        BTreeMap::from([
            ("operation_record".into(), "record".into()),
            ("operation_terminal_frame".into(), "frame".into()),
        ])
    );
    assert!(super::operation_source_properties(
        &record.operation_label,
        &[record.clone(), record.clone()],
        std::slice::from_ref(&common),
        std::slice::from_ref(&frame),
    )
    .is_empty());
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            &[],
            &[frame.clone(), frame],
        ),
        BTreeMap::from([("operation_record".into(), "record".into())])
    );
}

#[test]
fn solved_sketch_points_require_unique_exact_ownership_atomically() {
    let label = crate::native::features::FeatureOperationLabel {
        id: "nx:feature-history:operation-label#section-7".to_string(),
        section_link: "section".to_string(),
        ordinal: 7,
        value: "SKETCH".to_string(),
        object_indices: [None; 4],
        raw_object_indices: Default::default(),
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
        sketch_references: vec!["reference".to_string()],
        block_uses: vec!["block-use".to_string()],
        sketch_point_group: group.id.clone(),
        named_point: "named-point".to_string(),
        source_offsets: vec![52],
    };
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
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
            coordinate_pairs: &[],
        },
        &mut annotations,
        stream,
    )
    .expect("one exact point use projects a sketch");
    assert_eq!(ir.model.sketches[0].id, sketch);
    assert!(matches!(
        ir.model.sketch_entities[0].geometry,
        SketchGeometry::Point {
            position: Point2 { u: 12.5, v: -3.0 }
        }
    ));

    let mut rejected_ir = CadIr::empty(cadmpeg_ir::units::Units::default());
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
            coordinate_pairs: &[],
        },
        &mut rejected_annotations,
        rejected_stream,
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
        object_indices: [None; 4],
        raw_object_indices: Default::default(),
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
        crate::native::features::FeatureSketchPayloadScalar {
            id: id.to_string(),
            operation_label: label.id.clone(),
            construction_payload: "payload".to_string(),
            ordinal,
            field_code: 100,
            value,
            raw_value: [0; 8],
            payload_offset: ordinal as u64,
            source_offset,
        }
    };
    let scalars = [
        scalar("scalar-1", 0, 12.5, 51),
        scalar("scalar-2", 1, -3.0, 59),
    ];
    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
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
            coordinate_pairs: &[],
        },
        &mut annotations,
        stream,
    )
    .expect("a complete named payload point projects a sketch");
    assert_eq!(ir.model.sketches[0].id, sketch);
    assert_eq!(ir.model.sketch_entities.len(), 1);
    assert_eq!(
        ir.model.sketch_entities[0].native_ref.as_deref(),
        Some("point-group")
    );
    assert!(matches!(
        ir.model.sketch_entities[0].geometry,
        SketchGeometry::Point {
            position: Point2 { u: 12.5, v: -3.0 }
        }
    ));
}

#[test]
fn nx_native_feature_parameters_require_unique_resolved_names() {
    let expression = |id: &str, name: &str, text: &str| crate::native::om::Expression {
        id: id.to_string(),
        object_id: None,
        record: None,
        declaration: None,
        name: name.to_string(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: text.to_string(),
        value: None,
        source_entry: "entry".to_string(),
        source_table: "table".to_string(),
        source_offset: 0,
    };
    let parameter_use = |id: &str, expression: &str| crate::native::features::FeatureParameterUse {
        id: id.to_string(),
        operation_label: "operation".to_string(),
        expression: expression.to_string(),
        bindings: vec![format!("binding-{id}")],
        source_offsets: vec![0],
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
        ),
        cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "UNKNOWN OPERATION".to_string(),
            parameters: std::collections::BTreeMap::from([
                ("p1_length".to_string(), "p2_length * 2".to_string()),
                ("p2_length".to_string(), "12.5".to_string()),
            ]),
            properties: std::collections::BTreeMap::new(),
        }
    );
    assert!(matches!(
        super::non_boolean_feature_definition_with_parameters(
            "DELETE",
            &[],
            None,
            None,
            super::HoleProjection::default(),
            std::collections::BTreeMap::default(),
        ),
        cadmpeg_ir::features::FeatureDefinition::Native { kind, .. } if kind == "DELETE"
    ));
    assert!(matches!(
        super::non_boolean_feature_definition_with_parameters(
            "THRU_CURVE",
            &[],
            None,
            None,
            super::HoleProjection::default(),
            std::collections::BTreeMap::new(),
        ),
        cadmpeg_ir::features::FeatureDefinition::LoftUnresolved
    ));
    assert!(matches!(
        super::non_boolean_feature_definition_with_parameters(
            "SWP104",
            &[],
            None,
            None,
            super::HoleProjection::default(),
            std::collections::BTreeMap::new(),
        ),
        cadmpeg_ir::features::FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Unresolved(_),
            path: None,
            mode: cadmpeg_ir::features::SweepMode::Unresolved,
            ..
        }
    ));
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
            ),
            cadmpeg_ir::features::FeatureDefinition::SectionShape {
                first: cadmpeg_ir::features::BodySelection::Unresolved,
                second: cadmpeg_ir::features::BodySelection::Unresolved,
                approximate: None,
            }
        ));
    }
}

#[test]
fn nx_multi_instance_output_projects_as_an_unresolved_pattern() {
    assert!(matches!(
        super::non_boolean_feature_definition_with_parameters(
            "Multi Instance Output",
            &[],
            None,
            None,
            super::HoleProjection::default(),
            std::collections::BTreeMap::default(),
        ),
        cadmpeg_ir::features::FeatureDefinition::Pattern {
            seeds,
            pattern: cadmpeg_ir::features::PatternKind::Unresolved { form: None },
        } if seeds.is_empty()
    ));
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
        target_object_index: 7,
        raw_target_object_index: vec![7],
        target_source_offset: 1,
        tool_object_indices: vec![8],
        raw_tool_object_indices: vec![vec![8]],
        tool_source_offsets: vec![2],
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
        id: "nx:test:primary#0".into(),
        operation_label: boolean.operation_label.clone(),
        body_object_index: 7,
        raw_body_object_index: vec![7],
        source_offset: 3,
    };
    assert_eq!(
        super::native_result_body_identity(Some(&primary), Some(&boolean)),
        Some(("nx:test:primary#0".into(), "nx:test:primary#0".into(),))
    );
}

#[test]
fn boolean_target_output_requires_one_resolved_segment_body() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;

    let body = BodyId("nx:s0:body#0".into());
    let definition = FeatureDefinition::Combine {
        target: BodySelection::Resolved {
            bodies: vec![body.clone()],
            native: "target".into(),
        },
        tools: BodySelection::Unresolved,
        op: BooleanOp::Join,
        keep_tools: false,
    };
    assert_eq!(super::boolean_target_output(Some(&definition)), Some(body));

    let ambiguous = FeatureDefinition::Combine {
        target: BodySelection::Resolved {
            bodies: vec![BodyId("nx:s0:body#0".into()), BodyId("nx:s0:body#1".into())],
            native: "target".into(),
        },
        tools: BodySelection::Unresolved,
        op: BooleanOp::Join,
        keep_tools: false,
    };
    assert!(super::boolean_target_output(Some(&ambiguous)).is_none());
}

#[test]
fn topology_inferred_hole_axis_is_not_an_authored_direction() {
    use cadmpeg_ir::features::{FeatureDefinition, HolePlacement};
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
                        origin: Point3::new(1.0, 2.0, 3.0),
                        axis: Vector3::new(0.0, 0.0, 1.0),
                    }],
                    ..super::HoleProjection::default()
                },
                std::collections::BTreeMap::new(),
            ),
            FeatureDefinition::Hole {
                position: None,
                direction: None,
                placements,
                ..
            } if placements == [HolePlacement::Axis {
                origin: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            }]
        ));
    }
}

#[test]
fn complete_extrude_profile_projects_without_guessing_scalar_roles() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition, ProfileRef, Termination,
    };

    assert_eq!(
        super::extrude_feature_definition(
            Some("nx:profile#1"),
            None,
            BooleanOp::NewBody,
            &[cadmpeg_ir::topology::BodyKind::Solid],
        ),
        FeatureDefinition::Extrude {
            profile: ProfileRef::Native("nx:profile#1".to_string()),
            direction: cadmpeg_ir::features::ExtrudeDirection::Unresolved,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Unresolved,
                    draft: None,
                    offset: None,
                },
            },
            op: BooleanOp::NewBody,
            start: cadmpeg_ir::features::ExtrudeStart::Unresolved,
            direction_source: None,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        }
    );
    assert!(matches!(
        super::extrude_feature_definition(
            None,
            None,
            BooleanOp::Unresolved,
            &[cadmpeg_ir::topology::BodyKind::Sheet],
        ),
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            solid: Some(false),
            ..
        }
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
        FeatureDefinition::Extrude {
            profile: ProfileRef::Unresolved(_),
            solid: None,
            ..
        }
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

    let prior = super::FeatureId("prior-offset-writer".into());
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
        object_id: Some(key),
        record: None,
        declaration: None,
        name: format!("p{key}"),
        parameter_index: Some(key),
        qualifier: None,
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: key.to_string(),
        value: Some(f64::from(key)),
        source_entry: "part".into(),
        source_table: "table".into(),
        source_offset: u64::from(key),
    };
    let expressions = [expression(20), expression(21), expression(22)];
    let dimensions = crate::native::features::FeatureBlockDimensions {
        id: "dimensions".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        construction: "construction".into(),
        anchor_bindings: vec!["binding".into()],
        declarations: ["d20".into(), "d21".into(), "d22".into()],
        expressions: [
            expressions[0].id.clone(),
            expressions[1].id.clone(),
            expressions[2].id.clone(),
        ],
        values: [20.0, 21.0, 22.0],
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    super::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect();
    let parameter_references = dimensions
        .expressions
        .iter()
        .filter_map(|expression| super::expression_parameter_id(expression))
        .collect::<Vec<_>>();
    assert_eq!(
        super::parameter_owner_dependencies(&parameter_owners, &parameter_references),
        [ir.model.features[0].id.clone()]
    );
    assert_eq!(
        ir.model.features[0].source_content,
        ir.model
            .parameters
            .iter()
            .map(|parameter| {
                cadmpeg_ir::features::FeatureSourceContent::Parameter(parameter.id.clone())
            })
            .collect::<Vec<_>>()
    );
    super::attach_block_dimension_parameter_consumers(&mut ir, &[dimensions], &mut annotations);
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
fn feature_body_selection_retains_complete_input_local_identities_atomically() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let first = BodyId("nx:s2:body#3".to_string());
    let roots = BTreeMap::from([(94, 94), (122, 122)]);
    assert_eq!(
        super::feature_body_selection(
            &[94, 122],
            &roots,
            &BTreeMap::new(),
            "nx:om-object-indices#94,122".to_string(),
        )
        .selection,
        BodySelection::Local {
            bodies: vec![
                "nx:om-body-object#94".to_string(),
                "nx:om-body-object#122".to_string(),
            ],
            native: "nx:om-object-indices#94,122".to_string(),
        }
    );
    assert!(matches!(
        super::feature_body_selection(
            &[94, 123],
            &roots,
            &BTreeMap::new(),
            "nx:om-object-indices#94,123".to_string(),
        )
        .selection,
        BodySelection::Native(_)
    ));
    let aliases = BTreeMap::from([(94, 94), (150, 94)]);
    assert_eq!(
        super::feature_body_selection(
            &[94, 150],
            &aliases,
            &BTreeMap::new(),
            "nx:om-object-indices#94,150".to_string(),
        )
        .selection,
        BodySelection::Local {
            bodies: vec!["nx:om-body-object#94".to_string()],
            native: "nx:om-object-indices#94,150".to_string(),
        }
    );
    let bindings = BTreeMap::from([(94, vec![first.clone()])]);
    let segment_binding = |id: &str, stream_ordinal, body_object_index, alias| {
        crate::native::segments::SegmentBodyBinding {
            id: id.to_string(),
            stream_link: format!("stream-link#{stream_ordinal}"),
            stream_ordinal,
            stream_kind: "partition".to_string(),
            body_object_index,
            body_alias_object_index: alias,
            stream_role: 0,
            source_offset: 0,
        }
    };
    let segment_bindings = [segment_binding("binding#0", 0, 94, 150)];
    assert_eq!(
        super::feature_body_selection(
            &[94],
            &roots,
            &bindings,
            "nx:om-object-index#94".to_string(),
        )
        .selection,
        BodySelection::Resolved {
            bodies: vec![first.clone()],
            native: "nx:om-object-index#94".to_string(),
        }
    );
    assert_eq!(
        super::feature_body_outputs(94, &segment_bindings, &bindings),
        vec![first]
    );
    let ambiguous_body_bindings = BTreeMap::from([(
        94,
        vec![
            BodyId("nx:s2:body#3".to_string()),
            BodyId("nx:s2:body#4".to_string()),
        ],
    )]);
    assert!(
        super::feature_body_outputs(94, &segment_bindings, &ambiguous_body_bindings).is_empty()
    );
    assert!(super::feature_body_outputs(123, &segment_bindings, &bindings).is_empty());
    let ambiguous_bindings = [
        segment_binding("binding#0", 0, 94, 150),
        segment_binding("binding#1", 1, 94, 151),
    ];
    assert!(super::feature_body_outputs(94, &ambiguous_bindings, &bindings).is_empty());
}

#[test]
fn feature_body_selection_uses_complete_offset_store_proof_for_colliding_index() {
    use cadmpeg_ir::features::BodySelection;
    use std::collections::BTreeMap;

    let selection = super::feature_body_selection_with_offset_blocks(
        &[94],
        &BTreeMap::from([(94, 94)]),
        &BTreeMap::from([(94, "nx:om-data-blocks-3:block#94".to_string())]),
        &BTreeMap::new(),
        "nx:om-object-index#94".to_string(),
    );
    assert_eq!(
        selection.selection,
        BodySelection::Local {
            bodies: vec!["nx:om-data-blocks-3:block#94".to_string()],
            native: "nx:om-object-index#94".to_string(),
        }
    );
}
#[test]
fn native_primary_body_references_retain_only_proven_body_namespaces() {
    use crate::native::features::{
        FeatureBodyDataBlockUse, FeatureBodyReference, FeatureBodySegmentUse, FeatureInputBlock,
    };
    use crate::native::om::{DataBlock, DataBlockRole};

    let reference = |id: &str, operation_label: &str, body_object_index| FeatureBodyReference {
        id: id.to_string(),
        operation_label: operation_label.to_string(),
        body_object_index,
        raw_body_object_index: vec![body_object_index as u8],
        source_offset: 0,
    };
    let references = [
        reference("reference#segment", "operation#segment", 10),
        reference("reference#exact", "operation#exact", 99),
        reference("reference#missing", "operation#missing", 100),
        reference("reference#ambiguous", "operation#ambiguous", 101),
        reference("reference#duplicate-a", "operation#duplicate", 102),
        reference("reference#duplicate-b", "operation#duplicate", 103),
    ];
    let input = |id: &str, operation_label: &str, slot: u8, data_block: &str| FeatureInputBlock {
        id: id.to_string(),
        operation_label: operation_label.to_string(),
        input_slot: slot,
        object_index: u32::from(slot),
        raw_object_index: vec![slot],
        data_block: data_block.to_string(),
        source_offset: 0,
    };
    let blocks = [
        DataBlock {
            id: "block#exact-input".to_string(),
            section_ordinal: 2,
            block_ordinal: 3,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: String::new(),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
        DataBlock {
            id: "block#missing-input".to_string(),
            section_ordinal: 2,
            block_ordinal: 4,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: String::new(),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
        DataBlock {
            id: "block#ambiguous-input-1".to_string(),
            section_ordinal: 2,
            block_ordinal: 5,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: String::new(),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
        DataBlock {
            id: "block#ambiguous-input-2".to_string(),
            section_ordinal: 3,
            block_ordinal: 6,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 0,
            sha256: String::new(),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
    ];
    let data_block_uses = [FeatureBodyDataBlockUse {
        id: "data-block-use#exact".to_string(),
        feature_body_reference: references[1].id.clone(),
        data_block: "block#exact-output".to_string(),
    }];
    let inputs = [
        input("input#exact", "operation#exact", 0, "block#exact-input"),
        input(
            "input#missing",
            "operation#missing",
            0,
            "block#missing-input",
        ),
        input(
            "input#ambiguous-1",
            "operation#ambiguous",
            0,
            "block#ambiguous-input-1",
        ),
        input(
            "input#ambiguous-2",
            "operation#ambiguous",
            1,
            "block#ambiguous-input-2",
        ),
    ];

    let native = super::native_primary_body_references(
        &references,
        &data_block_uses,
        &[FeatureBodySegmentUse {
            id: "segment-use#exact".to_string(),
            feature_body_reference: references[1].id.clone(),
            segment_body_binding: "binding#exact".to_string(),
        }],
        &inputs,
        &blocks,
    );
    assert_eq!(native.get("operation#segment"), Some(&10));
    assert_eq!(native.get("operation#exact"), Some(&99));
    assert!(!native.contains_key("operation#missing"));
    assert!(!native.contains_key("operation#ambiguous"));
    assert!(!native.contains_key("operation#duplicate"));
}

#[test]
fn segment_bound_bodies_form_the_exact_retained_history_input() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::{BodyId, RegionId};
    use cadmpeg_ir::topology::{Body, BodyKind};

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    let bound = BodyId("nx:s2:body#3".to_string());
    ir.model.bodies.extend([
        Body {
            id: bound.clone(),
            kind: BodyKind::Solid,
            regions: vec![RegionId("region-2".to_string())],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
        Body {
            id: BodyId("nx:s3:body#4".to_string()),
            kind: BodyKind::Solid,
            regions: vec![RegionId("region-3".to_string())],
            transform: None,
            name: None,
            color: None,
            visible: None,
        },
    ]);
    let binding = crate::native::segments::SegmentBodyBinding {
        id: "nx:segment-body-bindings:binding#0".to_string(),
        stream_link: "nx:segment-stream-links:link#0".to_string(),
        stream_ordinal: 2,
        stream_kind: "partition".to_string(),
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 100,
    };
    let mut annotations = AnnotationBuilder::new();
    let stream = annotations.stream("nx:container");

    let id = super::attach_initial_segment_bodies(&mut ir, &[binding], &mut annotations, stream)
        .expect("one emitted body has an exact segment binding");

    assert_eq!(
        id,
        FeatureId("nx:feature-history:feature#initial-bodies".into())
    );
    assert_eq!(ir.model.features[0].outputs, std::slice::from_ref(&bound));
    assert_eq!(
        ir.model.features[0].definition,
        FeatureDefinition::BaseFeature {
            bodies: BodySelection::Resolved {
                bodies: vec![bound.clone()],
                native: "nx:segment-body-bindings".to_string(),
            },
        }
    );
    assert_eq!(
        crate::evaluation::evaluate_saved_body_census(&ir),
        crate::evaluation::BodyCensusEvaluation::Mismatch {
            rederived: vec![bound],
            saved: ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
        }
    );
}

#[test]
fn nx_boolean_retains_disjoint_current_and_input_local_bodies() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#0".to_string(),
        kind: crate::native::features::FeatureBooleanKind::Subtract,
        target_object_index: 94,
        raw_target_object_index: vec![94],
        target_source_offset: 0,
        tool_object_indices: vec![122],
        raw_tool_object_indices: vec![vec![122]],
        tool_source_offsets: vec![1],
        source_offset: 0,
    };
    let body = BodyId("nx:s18:body#3".to_string());
    let definition = super::boolean_feature_definition(
        &operation,
        &BTreeMap::from([(94, 94), (122, 122)]),
        &BooleanOffsetStoreResolution::None,
        &BTreeMap::from([(94, vec![body.clone()])]),
    );

    assert_eq!(
        definition,
        FeatureDefinition::Combine {
            target: BodySelection::Resolved {
                bodies: vec![body.clone()],
                native: "nx:om-object-index#94".to_string(),
            },
            tools: BodySelection::Local {
                bodies: vec!["nx:om-body-object#122".to_string()],
                native: "nx:om-object-indices#122".to_string(),
            },
            op: BooleanOp::Cut,
            keep_tools: false,
        }
    );
    let feature = Feature {
        id: FeatureId("feature".to_string()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![body],
        definition,
        native_ref: None,
    };
    assert!(!crate::decode::combine_definition_is_incomplete(&feature));
}

#[test]
fn nx_boolean_projects_unique_offset_store_body_blocks_as_local_bodies() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#offset".to_string(),
        operation_label: "operation#offset".to_string(),
        kind: crate::native::features::FeatureBooleanKind::Unite,
        target_object_index: 401,
        raw_target_object_index: Vec::new(),
        target_source_offset: 0,
        tool_object_indices: vec![402, 403],
        raw_tool_object_indices: vec![Vec::new(), Vec::new()],
        tool_source_offsets: vec![1, 2],
        source_offset: 0,
    };
    let blocks = BTreeMap::from([
        (401, "nx:om-data-blocks-3:block#401".to_string()),
        (402, "nx:om-data-blocks-3:block#402".to_string()),
        (403, "nx:om-data-blocks-3:block#403".to_string()),
    ]);

    assert_eq!(
        super::boolean_feature_definition(
            &operation,
            &BTreeMap::new(),
            &BooleanOffsetStoreResolution::Complete(blocks.clone()),
            &BTreeMap::new(),
        ),
        FeatureDefinition::Combine {
            target: BodySelection::Local {
                bodies: vec!["nx:om-data-blocks-3:block#401".to_string()],
                native: "nx:om-object-index#401".to_string(),
            },
            tools: BodySelection::Local {
                bodies: vec![
                    "nx:om-data-blocks-3:block#402".to_string(),
                    "nx:om-data-blocks-3:block#403".to_string(),
                ],
                native: "nx:om-object-indices#402,403".to_string(),
            },
            op: BooleanOp::Join,
            keep_tools: false,
        }
    );
}

#[test]
fn nx_boolean_writers_follow_selected_identity_namespace() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition, FeatureId};
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#writer-namespace".to_string(),
        operation_label: "nx:feature-history:operation-label#section-7".to_string(),
        kind: crate::native::features::FeatureBooleanKind::Unite,
        target_object_index: 401,
        raw_target_object_index: Vec::new(),
        target_source_offset: 0,
        tool_object_indices: vec![402],
        raw_tool_object_indices: vec![Vec::new()],
        tool_source_offsets: vec![1],
        source_offset: 0,
    };
    let blocks = BTreeMap::from([
        (401, "nx:om-data-blocks-3:block#401".to_string()),
        (402, "nx:om-data-blocks-3:block#402".to_string()),
    ]);
    let definition = super::boolean_feature_definition(
        &operation,
        &BTreeMap::new(),
        &BooleanOffsetStoreResolution::Complete(blocks.clone()),
        &BTreeMap::new(),
    );
    let FeatureDefinition::Combine { target, tools, .. } = &definition else {
        panic!("Boolean definition");
    };

    let native_prior = FeatureId("native-prior".to_string());
    let offset_prior = FeatureId("offset-prior".to_string());
    let mut history = super::BodyWriterHistory::default();
    history.record_writer(Some(401), None, &[], &native_prior);
    history.record_writer(None, Some(&blocks[&401]), &[], &offset_prior);
    history.record_writer(None, Some(&blocks[&402]), &[], &offset_prior);

    assert_eq!(
        super::boolean_participant_writer(target, 401, Some(&blocks), &BTreeMap::new(), &history,),
        Some(&offset_prior)
    );
    assert_eq!(
        super::boolean_participant_writer(tools, 402, Some(&blocks), &BTreeMap::new(), &history,),
        Some(&offset_prior)
    );
    assert_eq!(
        super::boolean_target_writer(&definition, 401),
        (None, Some("nx:om-data-blocks-3:block#401"))
    );

    let native_definition = FeatureDefinition::Combine {
        target: BodySelection::Native("nx:om-object-index#401".to_string()),
        tools: BodySelection::Native("nx:om-object-indices#402".to_string()),
        op: BooleanOp::Join,
        keep_tools: false,
    };
    assert_eq!(
        super::boolean_target_writer(&native_definition, 401),
        (Some(401), None)
    );
}

#[test]
fn nx_boolean_offset_store_resolution_requires_one_unique_store() {
    use crate::native::features::FeatureBooleanKind;
    use crate::native::om::{DataBlock, DataBlockRole};
    use std::collections::BTreeMap;

    let operation = crate::native::features::FeatureBooleanOperation {
        id: "boolean#offset-store".to_string(),
        operation_label: "nx:feature-history:operation-label#section-7".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 401,
        raw_target_object_index: Vec::new(),
        target_source_offset: 0,
        tool_object_indices: vec![402, 403],
        raw_tool_object_indices: vec![Vec::new(), Vec::new()],
        tool_source_offsets: vec![1, 2],
        source_offset: 0,
    };
    let block = |section_ordinal, block_ordinal| DataBlock {
        id: format!("nx:om-data-blocks-{section_ordinal}:block#{block_ordinal}"),
        section_ordinal,
        block_ordinal,
        role: DataBlockRole::Column,
        section_offset: 0,
        byte_len: 0,
        sha256: String::new(),
        stable_identity: None,
        source_entry: String::new(),
        source_offset: 0,
    };
    let same_store = vec![block(3, 401), block(3, 402), block(3, 403)];
    assert_eq!(
        crate::native::segments::boolean_offset_store_resolution(&operation, &same_store),
        crate::native::segments::BooleanOffsetStoreResolution::Complete(BTreeMap::from([
            (401, "nx:om-data-blocks-3:block#401".to_string()),
            (402, "nx:om-data-blocks-3:block#402".to_string()),
            (403, "nx:om-data-blocks-3:block#403".to_string()),
        ]))
    );
    let mixed_store = vec![block(3, 401), block(4, 402), block(4, 403)];
    assert!(matches!(
        crate::native::segments::boolean_offset_store_resolution(&operation, &mixed_store),
        crate::native::segments::BooleanOffsetStoreResolution::Unresolved
    ));
    assert!(matches!(
        crate::native::segments::boolean_offset_store_resolution(&operation, &[]),
        crate::native::segments::BooleanOffsetStoreResolution::None
    ));
    let mut control = block(3, 0);
    control.role = DataBlockRole::Control;
    let control_operation = crate::native::features::FeatureBooleanOperation {
        target_object_index: 0,
        tool_object_indices: vec![401, 402],
        ..operation.clone()
    };
    assert!(matches!(
        crate::native::segments::boolean_offset_store_resolution(
            &control_operation,
            &[control, block(3, 401), block(3, 402)],
        ),
        crate::native::segments::BooleanOffsetStoreResolution::Unresolved
    ));
}
