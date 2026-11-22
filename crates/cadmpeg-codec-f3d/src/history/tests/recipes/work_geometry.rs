// SPDX-License-Identifier: Apache-2.0

#[test]
fn three_point_recipe_vertices_must_define_the_solved_plane() {
    use cadmpeg_ir::math::Point3;

    let transform = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    assert!(crate::history::three_point_plane_matches(
        Some(transform),
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
        ],
    ));
    assert!(!crate::history::three_point_plane_matches(
        Some(transform),
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(20.0, 0.0, 0.0),
        ],
    ));
    assert!(!crate::history::three_point_plane_matches(
        Some(transform),
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 10.0, 1.0),
        ],
    ));
}

#[test]
fn work_point_vertex_recipe_resolves_common_historical_vertex() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
        AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::{
        dimensions::DesignRecipeReference,
        entity_header::DesignFeatureTimeline,
        feature::work_geometry::{
            DesignVertexRecipe, DesignWorkPointConstruction, DesignWorkPointInput,
            DesignWorkPointInputCarrier,
        },
    };
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::math::Point3;

    let stream = "f3d:Design/BulkStream.dat";
    let mut extrude = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        100,
    );
    extrude
        .try_edit(|draft| {
            draft.history_state_id = Some(4);
        })
        .unwrap();
    let reference = |face: i64| DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: face.to_string(),
        token_integer: 0,
        token_offset: 0,
        design_reference: 200,
        design_reference_offset: 0,
        candidate_faces: vec![
            FaceId::mint(crate::ids::brep_entity_id(face)).expect("identity grammar")
        ],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
        operand_offset: None,
    };
    let recipe = DesignVertexRecipe::try_new(
        crate::records::feature::work_geometry::DesignVertexRecipeDraft {
            record_index: 202,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("369".to_owned())
                .unwrap(),
            paired_byte_offset: 16,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 205,
            recipe_record_byte_offset: 32,
            recipe_id: format!("{stream}:construction-recipe#vertex"),
            recipe_prefix_offset: 43,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: vec![reference(10), reference(11), reference(12)],
            recipe_program_offset: 4,
            recipe_program: vec![0],
            resolution: None,
            next_record_index: 207,
            next_byte_offset: 200,
        },
    )
    .unwrap();
    let mut work_point = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        200,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkPoint(slot) =
        work_point.payload_mut()
    {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 201,
            point_record_byte_offset: 0,
            position: crate::test_support::reals([4.0, 3.0, 0.0]),
            position_offset: 0,
            rule: crate::records::feature::work_geometry::DesignWorkPointRule::try_from(
                crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex {
                    input: DesignWorkPointInput::try_new(
                        202,
                        0,
                        Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                            recipe,
                        })),
                    )
                    .unwrap(),
                },
            )
            .expect("compatible WorkPoint rule"),
            reference_type_offset: 0,
        });
    }
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let coedge = |coedge, owner_loop, edge| AsmHistoricalCoedge {
        coedge,
        owner_loop,
        edge,
        next: coedge,
        previous: coedge,
        radial_next: coedge,
    };
    let edge = |edge, start_vertex, end_vertex| AsmHistoricalEdge {
        edge,
        start_vertex,
        end_vertex,
    };
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11, 12],
        loops: vec![110, 111, 112],
        coedges: (1000..1009).collect(),
        edges: (2000..2009).collect(),
        vertices: vec![40, 41, 42, 43, 44, 45, 46],
        points: vec![50],
        face_loops: vec![
            relation(10, vec![110]),
            relation(11, vec![111]),
            relation(12, vec![112]),
        ],
        loop_coedges: vec![
            relation(110, vec![1000, 1001, 1002]),
            relation(111, vec![1003, 1004, 1005]),
            relation(112, vec![1006, 1007, 1008]),
        ],
        coedge_topology: vec![
            coedge(1000, 110, 2000),
            coedge(1001, 110, 2001),
            coedge(1002, 110, 2002),
            coedge(1003, 111, 2003),
            coedge(1004, 111, 2004),
            coedge(1005, 111, 2005),
            coedge(1006, 112, 2006),
            coedge(1007, 112, 2007),
            coedge(1008, 112, 2008),
        ],
        edge_vertices: vec![
            edge(2000, 40, 41),
            edge(2001, 41, 42),
            edge(2002, 42, 40),
            edge(2003, 40, 43),
            edge(2004, 43, 44),
            edge(2005, 44, 40),
            edge(2006, 40, 45),
            edge(2007, 45, 46),
            edge(2008, 46, 40),
        ],
        vertex_points: vec![AsmHistoricalCarrierBinding {
            entity: 40,
            carrier: 50,
        }],
        point_positions: vec![AsmHistoricalPoint {
            point: 50,
            position: Point3::new(40.0, 30.0, 0.0),
        }],
        ..AsmHistoricalTopology::default()
    };
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![(
            AsmDeltaState {
                id: "f3d:history:state#4".into(),
                parent: "f3d:history".into(),
                byte_offset: 0,
                state_id: 4,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: 0,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology),
            Vec::new(),
        )],
    );
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::identity::Located {
                    value: 100,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 200,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let mut scopes = vec![extrude, work_point];

    crate::history::bind_vertex_recipe_history(
        &mut scopes,
        std::slice::from_ref(&timeline),
        std::slice::from_ref(&history),
    )
    .expect("authored WorkPoint history");
    let construction = scopes[1]
        .work_point_construction()
        .expect("WorkPoint construction");
    let crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex { input } =
        construction.rule.form()
    else {
        unreachable!("test construction is vertex-based")
    };
    let Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) = input.carrier() else {
        unreachable!("test input carries a vertex recipe")
    };
    assert_eq!(
        recipe.resolution.map(|resolution| resolution.state_id),
        Some(4)
    );
    assert_eq!(
        recipe
            .resolution
            .map(crate::records::feature::work_geometry::DesignVertexResolution::vertex_slot),
        Some(40)
    );

    let mut ambiguous = scopes;
    let construction = ambiguous[1]
        .work_point_construction_mut()
        .expect("WorkPoint construction");
    let recipe = construction
        .rule
        .vertex_recipes_mut()
        .next()
        .expect("vertex recipe");
    recipe.recipe_references[0]
        .candidate_faces
        .push(FaceId::mint(crate::ids::brep_entity_id(11)).expect("identity grammar"));
    crate::history::bind_vertex_recipe_history(
        &mut ambiguous,
        std::slice::from_ref(&timeline),
        std::slice::from_ref(&history),
    )
    .expect("authored WorkPoint history");
    let construction = ambiguous[1]
        .work_point_construction()
        .expect("WorkPoint construction");
    let crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex { input } =
        construction.rule.form()
    else {
        unreachable!("test construction is vertex-based")
    };
    let Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) = input.carrier() else {
        unreachable!("test input carries a vertex recipe")
    };
    assert_eq!(recipe.resolution, None);
}

#[test]
fn feature_input_topology_projects_historical_vertices() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, UnresolvedFamily};

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:design:scope#work-point",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        7,
    );
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(4);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let feature = Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#work-point")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("WorkPoint".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPoint,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let mut history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![(
            AsmDeltaState {
                id: "f3d:history:state#4".into(),
                parent: "f3d:history".into(),
                byte_offset: 0,
                state_id: 4,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: 0,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology {
                vertices: vec![43, 59],
                ..AsmHistoricalTopology::default()
            }),
            Vec::new(),
        )],
    );

    let projected = crate::history::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        std::slice::from_mut(&mut history),
        &[],
        &std::collections::HashMap::new(),
    );
    let prefix = crate::ids::feature_input_prefix(&feature.id, 4);
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].vertices.as_slice(),
        [
            crate::ids::history_input_vertex_id(&prefix, 43),
            crate::ids::history_input_vertex_id(&prefix, 59),
        ]
    );
}
