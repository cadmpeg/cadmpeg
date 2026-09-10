// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use super::super::*;
use crate::records::topology::DesignConstructionOperandGroup;
use crate::records::topology::DesignConstructionOperandGroupFrame;
use crate::records::topology::DesignOperandRole;

#[test]
fn three_point_recipe_vertices_must_define_the_solved_plane() {
    use cadmpeg_ir::math::Point3;

    let transform = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    assert!(super::super::three_point_plane_matches(
        Some(transform),
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
        ],
    ));
    assert!(!super::super::three_point_plane_matches(
        Some(transform),
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(20.0, 0.0, 0.0),
        ],
    ));
    assert!(!super::super::three_point_plane_matches(
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
        AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology, AsmHistory,
    };
    use crate::records::feature::{
        DesignVertexRecipe, DesignWorkPointConstruction, DesignWorkPointInput,
        DesignWorkPointInputCarrier,
    };
    use crate::records::{DesignFeatureTimeline, DesignRecipeReference};
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::math::Point3;

    let stream = "f3d:Design/BulkStream.dat";
    let mut extrude = crate::records::feature::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::DesignFeatureKind::Extrude,
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
        token_offset: 0,
        design_reference: 200,
        design_reference_offset: 0,
        candidate_faces: vec![
            FaceId::mint(crate::ids::brep_entity_id(face)).expect("identity grammar")
        ],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let recipe = DesignVertexRecipe::try_new(crate::records::feature::DesignVertexRecipeDraft {
        record_index: 202,
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("369".to_owned()).unwrap(),
        paired_byte_offset: 16,
        paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),
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
    })
    .unwrap();
    let mut work_point = crate::records::feature::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::feature::DesignFeatureKind::WorkPoint,
        200,
    );
    if let crate::records::feature::DesignScopePayloadMut::WorkPoint(slot) =
        work_point.payload_mut()
    {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 201,
            point_record_byte_offset: 0,
            position: [4.0, 3.0, 0.0],
            position_offset: 0,
            rule: crate::records::feature::DesignWorkPointRule::try_from(
                crate::records::feature::DesignWorkPointRuleForm::Vertex {
                    input: DesignWorkPointInput::try_new(
                        crate::records::feature::DesignWorkPointInputDraft {
                            record_index: 202,
                            reference_offset: 0,
                            carrier: Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                                recipe,
                            })),
                        },
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
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![AsmDeltaState {
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
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
            transition: None,
        }],
    };
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::Located {
                    value: 100,
                    offset: 0,
                },
                crate::records::Located {
                    value: 200,
                    offset: 0,
                },
            ],
        ),
        crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let mut scopes = vec![extrude, work_point];

    super::super::bind_vertex_recipe_history(
        &mut scopes,
        std::slice::from_ref(&timeline),
        std::slice::from_ref(&history),
    )
    .expect("authored WorkPoint history");
    let construction = scopes[1]
        .work_point_construction()
        .expect("WorkPoint construction");
    let crate::records::feature::DesignWorkPointRuleForm::Vertex { input } =
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
            .map(crate::records::feature::DesignVertexResolution::vertex_slot),
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
    super::super::bind_vertex_recipe_history(
        &mut ambiguous,
        std::slice::from_ref(&timeline),
        std::slice::from_ref(&history),
    )
    .expect("authored WorkPoint history");
    let construction = ambiguous[1]
        .work_point_construction()
        .expect("WorkPoint construction");
    let crate::records::feature::DesignWorkPointRuleForm::Vertex { input } =
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
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
    use cadmpeg_ir::features::{Feature, FeatureDefinition, UnresolvedFamily};

    let mut scope = crate::records::feature::DesignParameterScope::empty(
        "f3d:design:scope#work-point",
        crate::records::feature::DesignFeatureKind::WorkPoint,
        7,
    );
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(4);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let feature = Feature {
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

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Unresolved {
                family: UnresolvedFamily::DatumPoint,
            },
        ),
        native_ref: Some(scope.id.clone()),
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![AsmDeltaState {
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
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Complete(
                AsmHistoricalTopology {
                    vertices: vec![43, 59],
                    ..AsmHistoricalTopology::default()
                },
            ),
            transition: None,
        }],
    };

    let projected = super::super::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        std::slice::from_ref(&history),
        &[],
    );
    let prefix = super::super::feature_input_prefix(&feature.id, 4);
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].vertices.as_slice(),
        [
            crate::ids::history_input_vertex_id(&prefix, 43),
            crate::ids::history_input_vertex_id(&prefix, 59),
        ]
    );
}

#[test]
fn surface_patch_recipe_uses_the_unique_common_boundary_edge() {
    use crate::history_records::{
        AsmHistoricalCoedge, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::topology::{
        DesignSurfacePatchRecipeClause, DesignSurfacePatchRecipeStructure,
    };
    use crate::records::DesignRecipeReference;
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let clause = |faces, edges| DesignSurfacePatchRecipeClause {
        fields: Vec::new(),
        face_reference_ordinals: faces,
        edge_reference_ordinals: edges,

        entries: Vec::new(),
    };
    let structure = DesignSurfacePatchRecipeStructure {
        clauses: [clause([1, 2], [0, 3]), clause([4, 1], [0, 5])],
    };
    let reference = |candidate_faces, candidate_edges| DesignRecipeReference {
        selector: 0,
        selector_offset: 0,
        token: String::new(),
        token_offset: 0,
        design_reference: 0,
        design_reference_offset: 0,
        candidate_faces,
        candidate_edges,
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let references = vec![
        reference(
            Vec::new(),
            vec![
                EdgeId::mint("test:model:edge#22").expect("identity grammar"),
                EdgeId::mint("test:model:edge#23").expect("identity grammar"),
            ],
        ),
        reference(
            vec![FaceId::mint("test:model:face#10").expect("identity grammar")],
            Vec::new(),
        ),
        reference(Vec::new(), Vec::new()),
        reference(Vec::new(), Vec::new()),
        reference(Vec::new(), Vec::new()),
        reference(Vec::new(), Vec::new()),
    ];
    let topology = AsmHistoricalTopology {
        faces: vec![10],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12],
        }],
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 12,
            owner_loop: 11,
            edge: 22,
            next: 12,
            previous: 12,
            radial_next: 12,
        }],
        ..Default::default()
    };
    assert_eq!(
        super::super::surface_patch_edge_operand_slot(Some(&structure), &references, &topology,),
        Some(22)
    );

    let mut ambiguous = topology.clone();
    ambiguous.loop_coedges[0].member_refs.push(13);
    ambiguous.coedge_topology.push(AsmHistoricalCoedge {
        coedge: 13,
        owner_loop: 11,
        edge: 23,
        next: 12,
        previous: 12,
        radial_next: 13,
    });
    assert_eq!(
        super::super::surface_patch_edge_operand_slot(Some(&structure), &references, &ambiguous,),
        None
    );
}

#[test]
fn external_body_candidate_requires_one_displayed_body_across_every_clause() {
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};

    let reference = |faces: &[&str]| crate::records::topology::DesignBodyRecipeReference {
        design_reference: 1,
        design_reference_offset: 25,
        form: 3,
        form_offset: 33,
        candidate_faces: faces
            .iter()
            .map(|face| FaceId::mint((*face).to_owned()).expect("identity grammar"))
            .collect(),
        preceding_candidate_faces: Vec::new(),
        preceding_body_slots: Vec::new(),
    };
    let mut operand = crate::records::topology::DesignBodyRecipeOperand::try_new(
        crate::records::topology::DesignBodyRecipeOperandDraft {
            id: "operand".into(),
            scope_record_index: 1,
            owner: crate::records::topology::DesignOperandOwner::ScopeReference {
                scope_reference_ordinal: 0,
            },
            record_index: 2,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("295".to_owned()).unwrap(),
            asset_id: crate::records::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![reference(&[
                "f3d:brep/current/brep:face#1",
                "f3d:brep/external/brep:face#1",
                "f3d:brep/cache/brep:face#1",
            ])],
            nested_record_index: 5,
            nested_record_index_offset: 38,
            recipe_id: "recipe".into(),
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: 6,
            next_byte_offset: 256,
        },
    )
    .unwrap();
    let body = |id: &str, region: &str, visible| Body {
        id: BodyId::mint(id).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint(region).expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible,
    };
    let bodies = [
        body(
            "f3d:brep/current/brep:body#1",
            "test:model:region#current-region",
            Some(true),
        ),
        body(
            "f3d:brep/external/brep:body#1",
            "test:model:region#external-region",
            Some(true),
        ),
        body(
            "f3d:brep/cache/brep:body#1",
            "test:model:region#cache-region",
            None,
        ),
    ];
    let regions = [
        Region {
            id: RegionId::mint("test:model:region#current-region").expect("identity grammar"),
            body: bodies[0].id.clone(),
            shells: vec![ShellId::mint("test:model:shell#current-shell").expect("identity grammar")],
        },
        Region {
            id: RegionId::mint("test:model:region#external-region").expect("identity grammar"),
            body: bodies[1].id.clone(),
            shells: vec![
                ShellId::mint("test:model:shell#external-shell").expect("identity grammar")
            ],
        },
        Region {
            id: RegionId::mint("test:model:region#cache-region").expect("identity grammar"),
            body: bodies[2].id.clone(),
            shells: vec![ShellId::mint("test:model:shell#cache-shell").expect("identity grammar")],
        },
    ];
    let shell = |id: &str, region: &str, face: &str| {
        Shell::with_face(
            ShellId::mint(id).expect("identity grammar"),
            RegionId::mint(region).expect("identity grammar"),
            FaceId::mint(face).expect("identity grammar"),
        )
    };
    let shells = [
        shell(
            "test:model:shell#current-shell",
            "test:model:region#current-region",
            "f3d:brep/current/brep:face#1",
        ),
        shell(
            "test:model:shell#external-shell",
            "test:model:region#external-region",
            "f3d:brep/external/brep:face#1",
        ),
        shell(
            "test:model:shell#cache-shell",
            "test:model:region#cache-region",
            "f3d:brep/cache/brep:face#1",
        ),
    ];

    assert_eq!(
        super::super::unique_external_body_candidate(
            &operand,
            Some("current"),
            &bodies,
            &regions,
            &shells,
        ),
        Some(bodies[1].id.clone())
    );

    operand
        .reference_bindings_mut()
        .next()
        .unwrap()
        .candidate_faces
        .retain(|face| !face.as_str().contains("/cache/"));
    let mut draft = operand.into_draft();
    let mut added = reference(&["f3d:brep/cache/brep:face#1"]);
    added.design_reference_offset = draft.byte_offset + 25 + draft.references.len() as u64 * 12;
    added.form_offset = added.design_reference_offset + 8;
    draft.references.push(added);
    draft.nested_record_index_offset = draft.byte_offset + 26 + draft.references.len() as u64 * 12;
    draft.asset_id_offset = draft.nested_record_index_offset + 18;
    operand = crate::records::topology::DesignBodyRecipeOperand::try_new(draft).unwrap();
    assert_eq!(
        super::super::unique_external_body_candidate(
            &operand,
            Some("current"),
            &bodies,
            &regions,
            &shells,
        ),
        None
    );
}

#[test]
fn body_recipe_history_resolves_the_complete_input_body_boundary() {
    use cadmpeg_ir::ids::FaceId;

    let mut scope = crate::records::feature::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::DesignFeatureKind::Extrude,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
        })
        .unwrap();
    let candidate = FaceId::mint("f3d:brep:entity#10").expect("identity grammar");
    let mut operands = vec![crate::records::topology::DesignBodyRecipeOperand::try_new(
        crate::records::topology::DesignBodyRecipeOperandDraft {
            id: "f3d:Design/BulkStream.dat:design-body-recipe-operand#21".into(),
            scope_record_index: 10,
            owner: crate::records::topology::DesignOperandOwner::Group {
                group_record_index: 20,
                group_member_ordinal: 0,
            },
            record_index: 21,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("365".to_owned()).unwrap(),
            asset_id: crate::records::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![crate::records::topology::DesignBodyRecipeReference {
                design_reference: 301,
                design_reference_offset: 25,
                form: 33,
                form_offset: 33,
                candidate_faces: vec![candidate.clone()],
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            }],
            nested_record_index: 24,
            nested_record_index_offset: 38,
            recipe_id: "recipe".into(),
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: 25,
            next_byte_offset: 256,
        },
    )
    .unwrap()];
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let topology = AsmHistoricalTopology {
        bodies: vec![1, 4],
        regions: vec![2, 5],
        shells: vec![3, 6],
        faces: vec![10, 11, 12, 20],
        surfaces: vec![100, 101, 102, 200],
        body_regions: vec![relation(1, vec![2]), relation(4, vec![5])],
        region_shells: vec![relation(2, vec![3]), relation(5, vec![6])],
        shell_faces: vec![relation(3, vec![10, 11, 12]), relation(6, vec![20])],
        shell_wire_edges: vec![relation(3, Vec::new()), relation(6, Vec::new())],
        shell_free_vertices: vec![relation(3, Vec::new()), relation(6, Vec::new())],
        face_loops: vec![
            relation(10, Vec::new()),
            relation(11, Vec::new()),
            relation(12, Vec::new()),
            relation(20, Vec::new()),
        ],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 102,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("f3d:Breps.BlobParts/BREP.input:asm-delta-state#{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition,
    };
    let previous = state(1, topology.clone(), None);
    let current = state(
        2,
        topology,
        Some(AsmHistoricalTransition {
            previous_state_id: Some(1),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        }),
    );
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![current, previous],
    };

    bind_body_recipe_operand_history_candidates(
        &mut operands,
        &[],
        std::slice::from_ref(&scope),
        std::slice::from_ref(&history),
    );

    assert_eq!(
        operands[0].references()[0].preceding_candidate_faces,
        [candidate]
    );
    assert_eq!(operands[0].references()[0].preceding_body_slots, [1]);
    assert_eq!(operands[0].resolved_face_slot, Some(10));
    assert_eq!(operands[0].resolved_body_state_id, Some(1));
    assert_eq!(operands[0].resolved_body_slot, Some(1));
    assert_eq!(operands[0].resolved_body_face_slots, [10, 11, 12]);
}

#[test]
fn complete_body_boundary_rejects_incomplete_or_ambiguous_incidence() {
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let topology = AsmHistoricalTopology {
        bodies: vec![1],
        regions: vec![2],
        shells: vec![3],
        faces: vec![10, 11],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, vec![10, 11])],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(complete_body_face_slots(&topology, 1), Some(vec![10, 11]));

    let mut incomplete = topology.clone();
    incomplete.shell_faces[0].member_refs.clear();
    assert_eq!(complete_body_face_slots(&incomplete, 1), None);

    let mut ambiguous = topology;
    ambiguous.shell_faces.push(relation(4, vec![10]));
    assert_eq!(complete_body_face_slots(&ambiguous, 1), None);
}

#[test]
fn direct_body_recipe_selection_resolves_compact_coil_target() {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, ScaleCenter, ScaleFactors,
    };
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};

    let scope = crate::records::feature::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::DesignFeatureKind::CoilPrimitive,
        10,
    );
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: group_id.into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("280".to_owned()).unwrap(),
            members: vec![crate::records::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                DesignOperandRole::BODIES_B,
            ),
            role_offset: 0,
            paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned()).unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let operand = crate::records::topology::DesignBodyRecipeOperand::try_new(
        crate::records::topology::DesignBodyRecipeOperandDraft {
            id: "f3d:Design/BulkStream.dat:design-body-recipe-operand#21".into(),
            scope_record_index: 10,
            owner: crate::records::topology::DesignOperandOwner::Group {
                group_record_index: 20,
                group_member_ordinal: 0,
            },
            record_index: 21,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("384".to_owned()).unwrap(),
            asset_id: crate::records::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![crate::records::topology::DesignBodyRecipeReference {
                design_reference: 301,
                design_reference_offset: 25,
                form: 33,
                form_offset: 33,
                candidate_faces: vec![FaceId::mint("f3d:brep:entity#7").expect("identity grammar")],
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            }],
            nested_record_index: 24,
            nested_record_index_offset: 38,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#23".into(),
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: 25,
            next_byte_offset: 256,
        },
    )
    .unwrap();
    let body = Body {
        id: BodyId::mint("f3d:brep:body#1").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("test:model:region#1").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
    };
    let region = Region {
        id: RegionId::mint("test:model:region#1").expect("identity grammar"),
        body: body.id.clone(),
        shells: vec![ShellId::mint("test:model:shell#1").expect("identity grammar")],
    };
    let shell = Shell::with_face(
        ShellId::mint("test:model:shell#1").expect("identity grammar"),
        region.id.clone(),
        FaceId::mint("f3d:brep:entity#7").expect("identity grammar"),
    );
    let inputs = super::super::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
    };
    let mut selection = BodySelection::Native(group_id.into());
    super::super::bind_direct_body_recipe_body_selection(&mut selection, &scope, &inputs);
    assert_eq!(
        selection,
        BodySelection::Resolved {
            bodies: vec![BodyId::mint("f3d:brep:body#1").expect("identity grammar")],
            native: group_id.into(),
        }
    );

    let recipe = crate::records::ConstructionRecipe {
        id: operand.recipe_id.clone(),
        byte_offset: 0,
        record_index_offset: None,
        kind: crate::records::ConstructionRecipeKind::Body,
        design: Some(crate::records::ConstructionRecipeDesign {
            id: crate::records::RecordedValue {
                value: "301".into(),
                offset: 0,
            },
            selector: Some(crate::records::ConstructionRecipeSelector {
                value: 9,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: 0,
    };
    let link = crate::records::PersistentDesignLink {
        id: "link".into(),
        target: cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
        design_id: "301".to_owned().try_into().unwrap(),

        design_reference: 9,
        ordinal: 0,
        is_current: true,
    };
    assert_eq!(
        super::super::body_recipe_link_candidate(
            &operand,
            std::slice::from_ref(&recipe),
            std::slice::from_ref(&link),
            std::slice::from_ref(&body),
        ),
        Some(body.id.clone())
    );

    let mut direct_operand = operand.clone();
    direct_operand.owner = crate::records::topology::DesignOperandOwner::ScopeReference {
        scope_reference_ordinal: 0,
    };
    let direct_inputs = super::super::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: &[],
        body_recipe_operands: std::slice::from_ref(&direct_operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
    };
    let native = format!(
        "{}:design-record#21",
        crate::ids::native_stream(&scope.id).expect("test scope stream")
    );
    let mut selection = BodySelection::NativeSet(vec![native.clone()].try_into().unwrap());
    super::super::bind_direct_body_recipe_body_selection(&mut selection, &scope, &direct_inputs);
    assert_eq!(
        selection,
        BodySelection::ResolvedSet {
            members: cadmpeg_ir::features::BodyMembers::try_from_parts(
                vec![body.id.clone()],
                vec![native],
            )
            .expect("valid body selection rows"),
        }
    );

    let mut scale_scope = scope.clone();
    scale_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::Scale
                .try_into()
                .unwrap();
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut scale_group = group.clone();
    scale_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    let scale_inputs = super::super::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scale_scope),
        groups: std::slice::from_ref(&scale_group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
    };
    let mut feature = Feature::new(
        FeatureId::mint("f3d:test:feature#scale").expect("identity grammar"),
        0,
        FeatureDefinition::Scale {
            bodies: BodySelection::Native(group_id.into()),
            center: Some(ScaleCenter::ModelOrigin),
            factors: ScaleFactors::Uniform(cadmpeg_ir::scalar::NonZeroReal::new(1.5).unwrap()),
        },
    );
    feature.native_ref = Some(scale_scope.id.clone());
    super::super::bind_feature_body_selections(std::slice::from_mut(&mut feature), &scale_inputs)
        .unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Scale {
            bodies: BodySelection::Resolved { ref bodies, ref native },
            ..
        } if bodies == &[body.id.clone()] && native == group_id
    ));

    let mut move_scope = scope;
    move_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::Move
                .try_into()
                .unwrap();
            draft.history_state_id = Some(42);
            draft.previous_history_state_id = Some(41);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let move_history = crate::history_records::AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: true,
        states: Vec::new(),
    };
    let move_inputs = super::super::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&move_scope),
        groups: std::slice::from_ref(&scale_group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: std::slice::from_ref(&move_history),
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
    };
    let mut move_feature = Feature::new(
        FeatureId::mint("f3d:test:feature#move").expect("identity grammar"),
        0,
        FeatureDefinition::MoveBody {
            bodies: BodySelection::Native(group_id.into()),
            translation: cadmpeg_ir::features::FiniteVector3::new(cadmpeg_ir::math::Vector3::new(
                1.0, 2.0, 3.0,
            ))
            .unwrap(),
            rotation: None,
            copies: 0,
        },
    );
    move_feature.native_ref = Some(move_scope.id.clone());
    super::super::bind_feature_body_selections(
        std::slice::from_mut(&mut move_feature),
        &move_inputs,
    )
    .unwrap();
    assert!(matches!(
        move_feature.evaluation.definition(),
        FeatureDefinition::MoveBody {
            bodies: BodySelection::Resolved { ref bodies, ref native },
            ..
        } if bodies == &[body.id.clone()] && native == group_id
    ));
}

#[test]
fn base_feature_body_selection_uses_active_transition_outputs() {
    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};
    use cadmpeg_ir::ids::BodyId;

    let mut feature = Feature {
        id: FeatureId::mint("test:model:feature#feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Native("native:scope".into()),
            },
            vec![
                BodyId::mint("test:model:body#2").expect("identity grammar"),
                BodyId::mint("test:model:body#1").expect("identity grammar"),
            ],
        )
        .unwrap(),
        native_ref: Some("native:scope".into()),
    };
    super::super::bind_base_feature_output_selection(&mut feature).unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::BaseFeature {
            bodies: BodySelection::Resolved { ref bodies, ref native }
        } if bodies == &[BodyId::mint("test:model:body#2").expect("identity grammar"), BodyId::mint("test:model:body#1").expect("identity grammar")]
            && native == "native:scope"
    ));
}

#[test]
fn opaque_history_span_retains_the_precise_framing_error() {
    let records = super::super::decode_history_records(
        &[0x33],
        0,
        None,
        "stream",
        "state",
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    );
    let [record] = records.as_slice() else {
        panic!("one opaque record");
    };
    assert_eq!(record.name(), "opaque_history_payload");
    assert!(record
        .framing_error()
        .is_some_and(|error| error.contains("byte 0") && error.contains("0x33")));
}

mod hem_carriers;
