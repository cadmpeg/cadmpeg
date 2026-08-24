// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]
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
    use crate::records::{
        DesignFeatureTimeline, DesignRecipeReference, DesignVertexRecipe,
        DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
        DesignWorkPointRule,
    };
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::math::Point3;

    let stream = "f3d:Design/BulkStream.dat";
    let mut extrude = crate::records::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        "Extrude",
        100,
    );
    extrude.history_state_id = Some(4);
    let reference = |face: i64| DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: face.to_string(),
        token_offset: 0,
        design_reference: 200,
        design_reference_offset: 0,
        candidate_faces: vec![FaceId(crate::ids::brep_entity_id(face))],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let recipe = DesignVertexRecipe {
        record_index: 202,
        byte_offset: 0,
        class_tag: "369".into(),
        paired_byte_offset: 1,
        paired_class_tag: "261".into(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 2,
        recipe_id: format!("{stream}:construction-recipe#vertex"),
        recipe_prefix_offset: 3,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: vec![reference(10), reference(11), reference(12)],
        recipe_program_offset: 4,
        recipe_program: vec![0],
        recipe_state_id: None,
        resolved_vertex_slot: None,
        next_record_index: 205,
        next_byte_offset: 5,
    };
    let mut work_point = crate::records::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        "WorkPoint",
        200,
    );
    work_point.work_point_construction = Some(DesignWorkPointConstruction {
        point_record_index: 201,
        point_record_byte_offset: 0,
        position: [4.0, 3.0, 0.0],
        position_offset: 0,
        rule: DesignWorkPointRule::Vertex {
            input: DesignWorkPointInput {
                record_index: 202,
                reference_offset: 0,
                carrier: Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                    recipe,
                })),
            },
        },
        reference_type_offset: 0,
    });
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
        stream_size: None,
        history_entry_count: None,
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
            record_table_complete: true,
            topology: Some(topology),
            transition: None,
        }],
    };
    let timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 1,
        context_record_index_offset: 0,
        item_count_offset: 0,
        item_record_indices: vec![100, 200],
        item_record_index_offsets: vec![0, 0],
    };
    let mut scopes = vec![extrude, work_point];

    super::super::bind_vertex_recipe_history(
        &mut scopes,
        std::slice::from_ref(&timeline),
        std::slice::from_ref(&history),
    )
    .expect("authored WorkPoint history");
    let construction = scopes[1]
        .work_point_construction
        .as_ref()
        .expect("WorkPoint construction");
    let DesignWorkPointRule::Vertex { input } = &construction.rule else {
        unreachable!("test construction is vertex-based")
    };
    let Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) = input.carrier.as_deref()
    else {
        unreachable!("test input carries a vertex recipe")
    };
    assert_eq!(recipe.recipe_state_id, Some(4));
    assert_eq!(recipe.resolved_vertex_slot, Some(40));

    let mut ambiguous = scopes;
    let construction = ambiguous[1]
        .work_point_construction
        .as_mut()
        .expect("WorkPoint construction");
    let DesignWorkPointRule::Vertex { input } = &mut construction.rule else {
        unreachable!("test construction is vertex-based")
    };
    let Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) = input.carrier.as_deref_mut()
    else {
        unreachable!("test input carries a vertex recipe")
    };
    recipe.recipe_references[0]
        .candidate_faces
        .push(FaceId(crate::ids::brep_entity_id(11)));
    super::super::bind_vertex_recipe_history(
        &mut ambiguous,
        std::slice::from_ref(&timeline),
        std::slice::from_ref(&history),
    )
    .expect("authored WorkPoint history");
    let construction = ambiguous[1]
        .work_point_construction
        .as_ref()
        .expect("WorkPoint construction");
    let DesignWorkPointRule::Vertex { input } = &construction.rule else {
        unreachable!("test construction is vertex-based")
    };
    let Some(DesignWorkPointInputCarrier::VertexRecipe { recipe }) = input.carrier.as_deref()
    else {
        unreachable!("test input carries a vertex recipe")
    };
    assert_eq!(recipe.recipe_state_id, None);
    assert_eq!(recipe.resolved_vertex_slot, None);
}

#[test]
fn feature_input_topology_projects_historical_vertices() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
    use cadmpeg_ir::features::{Feature, FeatureDefinition};

    let mut scope =
        crate::records::DesignParameterScope::empty("f3d:design:scope#work-point", "WorkPoint", 7);
    scope.previous_history_state_id = Some(4);
    let feature = Feature {
        id: cadmpeg_ir::features::FeatureId("f3d:model:feature#work-point".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("WorkPoint".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPointUnresolved,
        native_ref: Some(scope.id.clone()),
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        stream_size: None,
        history_entry_count: None,
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
            record_table_complete: true,
            topology: Some(AsmHistoricalTopology {
                vertices: vec![43, 59],
                ..AsmHistoricalTopology::default()
            }),
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
        projected[0].vertices,
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
    use crate::records::{
        DesignRecipeReference, DesignSurfacePatchRecipeClause, DesignSurfacePatchRecipeStructure,
    };
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let clause = |faces, edges| DesignSurfacePatchRecipeClause {
        fields: Vec::new(),
        face_reference_ordinals: faces,
        edge_reference_ordinals: edges,
        payload_entry_count: 0,
        entries: Vec::new(),
    };
    let structure = DesignSurfacePatchRecipeStructure {
        root: 2,
        clauses: vec![clause([1, 2], [0, 3]), clause([4, 1], [0, 5])],
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
            vec![EdgeId("edge#22".into()), EdgeId("edge#23".into())],
        ),
        reference(vec![FaceId("face#10".into())], Vec::new()),
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
fn hem_bend_carriers_prove_directional_gap_forms() {
    use crate::history_records::AsmHistoricalCylinder;
    use cadmpeg_ir::math::{Point3, Vector3};

    let cylinder = |radius| AsmHistoricalCylinder {
        surface: 1,
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        radius,
    };
    let flat_inner = cylinder(0.01);
    let flat_outer = cylinder(2.51);
    assert_eq!(
        super::super::hem_gap_length_form(&[&flat_inner, &flat_outer]),
        Some(super::super::HemGapLengthForm::Flat)
    );

    let open_inner = cylinder(1.25);
    let open_outer = cylinder(3.75);
    assert_eq!(
        super::super::hem_gap_length_form(&[&open_inner, &open_outer]),
        Some(super::super::HemGapLengthForm::Open)
    );

    assert_eq!(super::super::hem_gap_length_form(&[&flat_inner]), None);
}

#[test]
fn hem_carrier_offsets_prove_fold_direction() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder,
        AsmHistoricalEntityDelta, AsmHistoricalPlane, AsmHistoricalRelation, AsmHistoricalTopology,
        AsmHistoricalTopologyDelta, AsmHistoricalTransition,
    };
    use cadmpeg_ir::features::SheetMetalHemDirection;
    use cadmpeg_ir::math::{Point3, Vector3};

    let previous = AsmHistoricalTopology {
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 6,
            owner_loop: 5,
            edge: 7,
            next: 6,
            previous: 6,
            radial_next: 6,
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6],
        }],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 11,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 11,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        }],
        ..Default::default()
    };
    let transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: AsmHistoricalTopologyDelta {
            surfaces: AsmHistoricalEntityDelta {
                inserted: vec![12, 13],
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let cylinder = |origin| AsmHistoricalCylinder {
        surface: 12,
        origin,
        axis: Vector3::new(0.0, 1.0, 0.0),
        radius: 1.0,
    };
    let forward_first = cylinder(Point3::new(1.0, 0.0, 0.0));
    let forward_second = cylinder(Point3::new(2.0, 0.0, 0.0));
    assert_eq!(
        super::super::hem_direction_from_transition(
            7,
            &[&forward_first, &forward_second],
            &previous,
            &transition,
        ),
        Some(SheetMetalHemDirection::Forward)
    );

    let reverse_first = cylinder(Point3::new(-1.0, 0.0, 0.0));
    let reverse_second = cylinder(Point3::new(-2.0, 0.0, 0.0));
    assert_eq!(
        super::super::hem_direction_from_transition(
            7,
            &[&reverse_first, &reverse_second],
            &previous,
            &transition,
        ),
        Some(SheetMetalHemDirection::Reverse)
    );

    let zero_offset = cylinder(Point3::new(0.0, 0.0, 0.0));
    assert_eq!(
        super::super::hem_direction_from_transition(
            7,
            &[&zero_offset, &forward_second],
            &previous,
            &transition,
        ),
        None
    );
}

#[test]
fn external_body_candidate_requires_one_displayed_body_across_every_clause() {
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};

    let reference = |faces: &[&str]| crate::records::DesignBodyRecipeReference {
        design_reference: 1,
        design_reference_offset: 0,
        form: 3,
        form_offset: 0,
        candidate_faces: faces
            .iter()
            .map(|face| FaceId((*face).to_owned()))
            .collect(),
        preceding_candidate_faces: Vec::new(),
        preceding_body_slots: Vec::new(),
    };
    let mut operand = crate::records::DesignBodyRecipeOperand {
        id: "operand".into(),
        scope_record_index: 1,
        owner: crate::records::DesignBodyRecipeOperandOwner::ScopeReference {
            scope_reference_ordinal: 0,
        },
        record_index: 2,
        byte_offset: 0,
        class_tag: "295".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        selector_tail: None,
        selector_tail_offset: None,
        references: vec![reference(&[
            "f3d:brep/current/face#1",
            "f3d:brep/external/face#1",
            "f3d:brep/cache/face#1",
        ])],
        nested_record_index: 3,
        nested_record_index_offset: 0,
        recipe_id: "recipe".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 4,
        next_byte_offset: 0,
    };
    let body = |id: &str, region: &str, visible| Body {
        id: BodyId(id.into()),
        kind: BodyKind::Solid,
        regions: vec![RegionId(region.into())],
        transform: None,
        name: None,
        color: None,
        visible,
    };
    let bodies = [
        body("f3d:brep/current/body#1", "current-region", Some(true)),
        body("f3d:brep/external/body#1", "external-region", Some(true)),
        body("f3d:brep/cache/body#1", "cache-region", None),
    ];
    let regions = [
        Region {
            id: RegionId("current-region".into()),
            body: bodies[0].id.clone(),
            shells: vec![ShellId("current-shell".into())],
        },
        Region {
            id: RegionId("external-region".into()),
            body: bodies[1].id.clone(),
            shells: vec![ShellId("external-shell".into())],
        },
        Region {
            id: RegionId("cache-region".into()),
            body: bodies[2].id.clone(),
            shells: vec![ShellId("cache-shell".into())],
        },
    ];
    let shell = |id: &str, region: &str, face: &str| Shell {
        id: ShellId(id.into()),
        region: RegionId(region.into()),
        faces: vec![FaceId(face.into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    };
    let shells = [
        shell("current-shell", "current-region", "f3d:brep/current/face#1"),
        shell(
            "external-shell",
            "external-region",
            "f3d:brep/external/face#1",
        ),
        shell("cache-shell", "cache-region", "f3d:brep/cache/face#1"),
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

    operand.references[0]
        .candidate_faces
        .retain(|face| !face.0.contains("/cache/"));
    operand
        .references
        .push(reference(&["f3d:brep/cache/face#1"]));
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

    let mut scope = crate::records::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        "Extrude",
        10,
    );
    scope.history_state_id = Some(2);
    let candidate = FaceId("f3d:brep:entity#10".into());
    let mut operands = vec![crate::records::DesignBodyRecipeOperand {
        id: "f3d:Design/BulkStream.dat:design-body-recipe-operand#21".into(),
        scope_record_index: 10,
        owner: crate::records::DesignBodyRecipeOperandOwner::Group {
            group_record_index: 20,
            group_member_ordinal: 0,
        },
        record_index: 21,
        byte_offset: 0,
        class_tag: "365".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        selector_tail: None,
        selector_tail_offset: None,
        references: vec![crate::records::DesignBodyRecipeReference {
            design_reference: 301,
            design_reference_offset: 0,
            form: 33,
            form_offset: 0,
            candidate_faces: vec![candidate.clone()],
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 24,
        nested_record_index_offset: 0,
        recipe_id: "recipe".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 25,
        next_byte_offset: 0,
    }];
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
        record_table_complete: true,
        topology: Some(topology),
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
        stream_size: None,
        history_entry_count: None,
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
        operands[0].references[0].preceding_candidate_faces,
        [candidate]
    );
    assert_eq!(operands[0].references[0].preceding_body_slots, [1]);
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

    let scope = crate::records::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        "CoilPrimitive",
        10,
    );
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = crate::records::DesignConstructionOperandGroup {
        id: group_id.into(),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 20,
        byte_offset: 0,
        class_tag: "280".into(),
        members: vec![21],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "259".into(),
        paired_byte_offset: 0,
    };
    let operand = crate::records::DesignBodyRecipeOperand {
        id: "f3d:Design/BulkStream.dat:design-body-recipe-operand#21".into(),
        scope_record_index: 10,
        owner: crate::records::DesignBodyRecipeOperandOwner::Group {
            group_record_index: 20,
            group_member_ordinal: 0,
        },
        record_index: 21,
        byte_offset: 0,
        class_tag: "384".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        selector_tail: None,
        selector_tail_offset: None,
        references: vec![crate::records::DesignBodyRecipeReference {
            design_reference: 301,
            design_reference_offset: 0,
            form: 33,
            form_offset: 0,
            candidate_faces: vec![FaceId("f3d:brep:entity#7".into())],
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 22,
        nested_record_index_offset: 0,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#23".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 24,
        next_byte_offset: 0,
    };
    let body = Body {
        id: BodyId("f3d:brep:body#1".into()),
        kind: BodyKind::Solid,
        regions: vec![RegionId("region#1".into())],
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
    };
    let region = Region {
        id: RegionId("region#1".into()),
        body: body.id.clone(),
        shells: vec![ShellId("shell#1".into())],
    };
    let shell = Shell {
        id: ShellId("shell#1".into()),
        region: region.id.clone(),
        faces: vec![FaceId("f3d:brep:entity#7".into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    };
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
            bodies: vec![BodyId("f3d:brep:body#1".into())],
            native: group_id.into(),
        }
    );

    let recipe = crate::records::ConstructionRecipe {
        id: operand.recipe_id.clone(),
        byte_offset: 0,
        record_index_offset: None,
        kind: crate::records::ConstructionRecipeKind::Body,
        design_id: Some("301".into()),
        design_id_offset: None,
        design_selector: Some(crate::records::ConstructionRecipeSelector {
            value: 9,
            byte_offset: 0,
        }),
        recipe_index: 0,
        record_index: 0,
    };
    let link = crate::records::PersistentDesignLink {
        id: "link".into(),
        target: cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
        design_id: "301".into(),
        entity_kind: 3,
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
    direct_operand.owner = crate::records::DesignBodyRecipeOperandOwner::ScopeReference {
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
    let mut selection = BodySelection::NativeSet(vec![native.clone()]);
    super::super::bind_direct_body_recipe_body_selection(&mut selection, &scope, &direct_inputs);
    assert_eq!(
        selection,
        BodySelection::ResolvedSet {
            bodies: vec![body.id.clone()],
            native: vec![native],
        }
    );

    let mut scale_scope = scope.clone();
    scale_scope.kind = "Scale".into();
    scale_scope.previous_history_state_id = Some(7);
    let mut scale_group = group.clone();
    scale_group.role = 0x0000_0004_0000_0000;
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
        FeatureId("f3d:feature#scale".into()),
        0,
        FeatureDefinition::Scale {
            bodies: BodySelection::Native(group_id.into()),
            center: Some(ScaleCenter::ModelOrigin),
            factors: ScaleFactors {
                uniform: Some(1.5),
                x: None,
                y: None,
                z: None,
            },
        },
    );
    feature.native_ref = Some(scale_scope.id.clone());
    super::super::bind_feature_body_selections(std::slice::from_mut(&mut feature), &scale_inputs);
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Scale {
            bodies: BodySelection::Resolved { ref bodies, ref native },
            ..
        } if bodies == &[body.id.clone()] && native == group_id
    ));

    let mut move_scope = scope;
    move_scope.kind = "Move".into();
    move_scope.history_state_id = Some(42);
    move_scope.previous_history_state_id = Some(41);
    let move_history = crate::history_records::AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        stream_size: None,
        history_entry_count: None,
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
        FeatureId("f3d:feature#move".into()),
        0,
        FeatureDefinition::MoveBody {
            bodies: BodySelection::Native(group_id.into()),
            translation: cadmpeg_ir::math::Vector3::new(1.0, 2.0, 3.0),
            rotation: None,
            copies: 0,
        },
    );
    move_feature.native_ref = Some(move_scope.id.clone());
    super::super::bind_feature_body_selections(
        std::slice::from_mut(&mut move_feature),
        &move_inputs,
    );
    assert!(matches!(
        move_feature.definition,
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
        id: FeatureId("feature".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![BodyId("body:2".into()), BodyId("body:1".into())],
        definition: FeatureDefinition::BaseFeature {
            bodies: BodySelection::Native("native:scope".into()),
        },
        native_ref: Some("native:scope".into()),
    };
    super::super::bind_base_feature_output_selection(&mut feature);
    assert!(matches!(
        feature.definition,
        FeatureDefinition::BaseFeature {
            bodies: BodySelection::Resolved { ref bodies, ref native }
        } if bodies == &[BodyId("body:2".into()), BodyId("body:1".into())]
            && native == "native:scope"
    ));
}

#[test]
fn opaque_history_span_retains_the_precise_framing_error() {
    let records = super::super::decode_history_records(&[0x33], 0, None, "stream", "state", 8);
    let [record] = records.as_slice() else {
        panic!("one opaque record");
    };
    assert_eq!(record.name, "opaque_history_payload");
    assert!(record
        .framing_error
        .as_deref()
        .is_some_and(|error| error.contains("byte 0") && error.contains("0x33")));
}

#[test]
fn split_face_targets_bind_from_a_transition_predecessor() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
    use crate::records::{
        ConstructionRecipeKind, DesignConstructionOperandGroup,
        DesignConstructionOperandGroupFrame, DesignFaceOperand, DesignParameterScope,
    };
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, SplitFaceTool,
    };
    use cadmpeg_ir::ids::FaceId;

    let scope_id = "f3d:Design/BulkStream.dat:scope#42".to_string();
    let group_id = "f3d:Design/BulkStream.dat:operand-group#100".to_string();
    let face_id = FaceId("f3d:brep:entity#7".into());
    let mut scope = DesignParameterScope::empty(&scope_id, "SplitFace", 42);
    scope.history_state_id = Some(2);

    let group = DesignConstructionOperandGroup {
        id: group_id.clone(),
        scope_record_index: 42,
        scope_reference_ordinal: 2,
        record_index: 100,
        byte_offset: 1000,
        class_tag: "297".into(),
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![1010],
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 1008,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 1020,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 1024,
            variant: false,
        },
        role: 0x0000_0010_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 1030,
        paired_class_tag: "259".into(),
        paired_byte_offset: 1100,
    };
    let operand = DesignFaceOperand {
        id: "f3d:Design/BulkStream.dat:design-face-operand#200".into(),
        scope_record_index: 42,
        scope_reference_ordinal: 3,
        group_record_index: Some(100),
        group_member_ordinal: Some(0),
        record_index: 200,
        byte_offset: 1200,
        class_tag: "297".into(),
        paired_byte_offset: 1300,
        paired_class_tag: "259".into(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 1400,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
        recipe_prefix_offset: 1411,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1420,
        recipe_program: Vec::new(),
        recipe_node_offsets: Vec::new(),
        recipe_nodes: Vec::new(),
        candidate_faces: vec![face_id.clone()],
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: vec![face_id.clone()],
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 204,
        next_byte_offset: 1500,
    };
    let state = |state_id, transition| AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
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
        record_table_complete: true,
        topology: Some(AsmHistoricalTopology::default()),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        stream_size: None,
        history_entry_count: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(
                2,
                Some(crate::history_records::AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: Default::default(),
                    topology: Default::default(),
                }),
            ),
            state(1, None),
        ],
    };
    let mut features = vec![Feature {
        id: FeatureId("f3d:feature#42".into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: Some("SplitFace".into()),
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::SplitFace {
            targets: FaceSelection::Native(group_id.clone()),
            tool: SplitFaceTool::Plane {
                plane: FeatureId("f3d:feature#plane".into()),
            },
        },
        native_ref: Some(scope_id),
    }];

    super::super::bind_feature_face_selections(
        &mut features,
        &mut [],
        &[scope],
        &[group],
        &[operand],
        &[],
        &[],
        &[history],
    );

    assert!(matches!(
        &features[0].definition,
        FeatureDefinition::SplitFace {
            targets: FaceSelection::Resolved { faces, native },
            ..
        } if faces == &[face_id] && native == &group_id
    ));
}

#[test]
fn thread_face_group_uses_first_reference_transition_candidates() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalTopology,
        AsmHistoricalTransition, AsmHistory,
    };
    use crate::records::{
        ConstructionRecipeKind, DesignConstructionOperandGroup,
        DesignConstructionOperandGroupFrame, DesignFaceOperand, DesignParameterScope,
        DesignRecipeReference, DesignThreadConstruction, DesignThreadForm,
    };
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let face = |slot| FaceId(format!("f3d:brep:entity#{slot}"));
    let scope_id = "f3d:Design/BulkStream.dat:scope#42";
    let mut scope = DesignParameterScope::empty(scope_id, "Thread", 42);
    scope.history_state_id = Some(2);
    scope.previous_history_state_id = Some(1);

    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
        scope_record_index: 42,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1_000,
        class_tag: "297".into(),
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![1_010],
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 1_008,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 1_020,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 1_024,
            variant: false,
        },
        role: 0x0000_0010_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 1_030,
        paired_class_tag: "259".into(),
        paired_byte_offset: 1_100,
    };
    let reference = |token: &str, design_reference, candidates: &[i64]| DesignRecipeReference {
        selector: 1,
        selector_offset: 1_411,
        token: token.into(),
        token_offset: 1_415,
        design_reference,
        design_reference_offset: 1_420,
        candidate_faces: candidates.iter().copied().map(face).collect(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let operand = DesignFaceOperand {
        id: "f3d:Design/BulkStream.dat:design-face-operand#200".into(),
        scope_record_index: 42,
        scope_reference_ordinal: 1,
        group_record_index: Some(100),
        group_member_ordinal: Some(0),
        record_index: 200,
        byte_offset: 1_200,
        class_tag: "297".into(),
        paired_byte_offset: 1_300,
        paired_class_tag: "259".into(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 1_400,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
        recipe_prefix_offset: 1_411,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: vec![reference("3", 203, &[7, 8]), reference("-1", 199, &[9, 10])],
        recipe_kind: ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 1_430,
        recipe_program: vec![0, -1, 2],
        recipe_node_offsets: Vec::new(),
        recipe_nodes: Vec::new(),
        candidate_faces: [7, 8, 9, 10].into_iter().map(face).collect(),
        unreferenced_candidate_faces: [9, 10].into_iter().map(face).collect(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 204,
        next_byte_offset: 1_500,
    };

    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: Default::default(),
    };
    transition.topology.faces.updated.push(7);
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: (state_id == 2).then_some(1),
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology: Some(topology),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        stream_size: None,
        history_entry_count: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(2, AsmHistoricalTopology::default(), Some(transition)),
            state(
                1,
                AsmHistoricalTopology {
                    faces: vec![7, 8, 9, 10],
                    persistent_subentity_tags: [
                        (7, "3", 203),
                        (8, "3", 203),
                        (9, "-1", 199),
                        (10, "-1", 199),
                    ]
                    .into_iter()
                    .map(|(entity_ref, token, design_reference)| {
                        crate::history_records::AsmHistoricalPersistentSubentityTag {
                            entity_kind: AsmHistoricalEntityKind::Face,
                            entity_ref,
                            selector: 17,
                            token: token.into(),
                            design_references: vec![design_reference],
                            ordinal: 0,
                        }
                    })
                    .collect(),
                    ..AsmHistoricalTopology::default()
                },
                None,
            ),
        ],
    };

    let mut operands = vec![operand.clone()];
    bind_face_operand_history_candidates(
        &mut operands,
        std::slice::from_ref(&scope),
        std::slice::from_ref(&group),
        &[],
        std::slice::from_ref(&history),
        &HashMap::new(),
    );
    assert_eq!(operands[0].preceding_candidate_faces, [face(7), face(8)]);
    assert_eq!(operands[0].changed_candidate_faces, [face(7)]);
    assert_eq!(operands[0].resolved_face_slots, [7]);

    let mut cylinder_scope = scope.clone();
    cylinder_scope.thread_construction = Some(DesignThreadConstruction {
        form: DesignThreadForm::Standard,
        designation_offset: 0,
        designation: "M4x0.7".into(),
        nominal_size_text: "4.0".into(),
        nominal_size: 4.0,
        profile: "ISO Metric profile".into(),
        major_diameter: 0.4,
        minor_diameter: 0.2,
        pitch: 0.07,
        pitch_diameter: 0.3,
        trailing_reference_record_index: None,
        trailing_reference_offset: None,
        face_group_record_indices: vec![100],
    });
    let mut cylinder_operand = operand.clone();
    cylinder_operand.recipe_references[0].candidate_faces =
        vec![FaceId("f3d:brep/input/brep:entity#999".into())];
    cylinder_operand.candidate_faces = cylinder_operand.recipe_references[0]
        .candidate_faces
        .clone();
    let mut cylinder_history = history.clone();
    cylinder_history.id = "f3d:Breps.BlobParts/BREP.input:asm-history#1".into();
    let cylinder_topology = cylinder_history.states[1]
        .topology
        .as_mut()
        .expect("preceding topology");
    cylinder_topology.face_surfaces = vec![
        AsmHistoricalCarrierBinding {
            entity: 7,
            carrier: 70,
        },
        AsmHistoricalCarrierBinding {
            entity: 8,
            carrier: 80,
        },
    ];
    cylinder_topology.surface_cylinders = vec![
        AsmHistoricalCylinder {
            surface: 70,
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 1.5,
        },
        AsmHistoricalCylinder {
            surface: 80,
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 3.0,
        },
    ];
    let mut cylinder_operands = vec![cylinder_operand];
    bind_face_operand_history_candidates(
        &mut cylinder_operands,
        std::slice::from_ref(&cylinder_scope),
        std::slice::from_ref(&group),
        &[],
        std::slice::from_ref(&cylinder_history),
        &HashMap::new(),
    );
    assert_eq!(cylinder_operands[0].resolved_face_slots, [7]);

    let mut stale_active_operand = cylinder_operands[0].clone();
    stale_active_operand.recipe_references[0]
        .candidate_faces
        .push(FaceId("f3d:brep/input/brep:entity#998".into()));
    let mut stale_active_operands = vec![stale_active_operand];
    bind_face_operand_history_candidates(
        &mut stale_active_operands,
        std::slice::from_ref(&cylinder_scope),
        std::slice::from_ref(&group),
        &[],
        std::slice::from_ref(&cylinder_history),
        &HashMap::new(),
    );
    assert_eq!(stale_active_operands[0].resolved_face_slots, [7]);

    let mut ambiguous_geometry_history = cylinder_history;
    ambiguous_geometry_history.states[0]
        .transition
        .as_mut()
        .expect("result transition")
        .topology
        .faces
        .updated
        .push(8);
    ambiguous_geometry_history.states[1]
        .topology
        .as_mut()
        .expect("preceding topology")
        .surface_cylinders[1]
        .radius = 1.6;
    let mut ambiguous_geometry_operands = vec![cylinder_operands.remove(0)];
    bind_face_operand_history_candidates(
        &mut ambiguous_geometry_operands,
        &[cylinder_scope],
        std::slice::from_ref(&group),
        &[],
        &[ambiguous_geometry_history],
        &HashMap::new(),
    );
    assert!(ambiguous_geometry_operands[0]
        .resolved_face_slots
        .is_empty());

    let mut unrelated_group = group;
    unrelated_group.role = 0x0000_0011_0000_0000;
    let mut rejected = vec![operand];
    bind_face_operand_history_candidates(
        &mut rejected,
        &[scope],
        &[unrelated_group],
        &[],
        &[history],
        &HashMap::new(),
    );
    assert_eq!(rejected[0].preceding_candidate_faces, [face(9), face(10)]);
    assert!(rejected[0].changed_candidate_faces.is_empty());
    assert!(rejected[0].resolved_face_slots.is_empty());
}

#[test]
fn history_binding_budget_charges_materialized_state_tables() {
    let mut limits = cadmpeg_core::decode::ResourceLimits::desktop();
    limits.max_materialized_bytes = 1920;
    assert!(!complete_table_binding_budget_exceeded([5, 5], &limits));
    assert!(complete_table_binding_budget_exceeded([10, 1], &limits));
    assert!(complete_table_binding_budget_exceeded(
        [usize::MAX, 1],
        &limits,
    ));

    let desktop = cadmpeg_core::decode::ResourceLimits::desktop();
    let service = cadmpeg_core::decode::ResourceLimits::service();
    assert!(!complete_table_binding_budget_exceeded(
        [18_000_000],
        &desktop,
    ));
    assert!(complete_table_binding_budget_exceeded(
        [18_000_000],
        &service,
    ));
}

#[test]
fn unresolved_new_body_sweep_mode_follows_output_body_kind() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, SweepMode, SweepSection};
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};

    let body = |id: &str, kind| Body {
        id: BodyId(id.into()),
        kind,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let sweep = |id: &str, outputs| Feature {
        id: FeatureId(id.into()),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs,
        definition: FeatureDefinition::Sweep {
            section: SweepSection::Unresolved(None),
            sections: Vec::new(),
            path: None,
            mode: SweepMode::Unresolved,
            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist: None,
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale: None,
            allow_multi_profile_faces: None,
        },
        native_ref: None,
    };
    let bodies = [
        body("sheet", BodyKind::Sheet),
        body("solid", BodyKind::Solid),
    ];
    let mut features = [
        sweep("sheet-sweep", vec![BodyId("sheet".into())]),
        sweep("solid-sweep", vec![BodyId("solid".into())]),
        sweep(
            "mixed-sweep",
            vec![BodyId("sheet".into()), BodyId("solid".into())],
        ),
        sweep("missing-sweep", vec![BodyId("missing".into())]),
    ];

    bind_sweep_result_modes(&mut features, &bodies);

    let modes = features.map(|feature| match feature.definition {
        FeatureDefinition::Sweep { mode, .. } => mode,
        _ => unreachable!(),
    });
    assert_eq!(modes[0], SweepMode::Surface);
    assert_eq!(
        modes[1],
        SweepMode::Solid {
            op: cadmpeg_ir::features::BooleanOp::NewBody
        }
    );
    assert_eq!(modes[2], SweepMode::Unresolved);
    assert_eq!(modes[3], SweepMode::Unresolved);
}

#[test]
fn historical_brep_source_qualifies_state_local_candidates() {
    assert_eq!(
        historical_brep_source("f3d:asset/Breps.BlobParts/BREP.example.smbh:asm-delta-state#42"),
        Some("example.smbh")
    );
    assert_eq!(historical_brep_source("f3d:unqualified:state#42"), None);
}

#[test]
fn legacy_extrude_face_lane_prefers_history_then_source_identity() {
    use crate::history_records::AsmHistoricalTopology;
    use cadmpeg_ir::ids::FaceId;
    use std::collections::HashSet;

    let source_face = |source: &str, slot| FaceId(format!("f3d:brep/{source}/entity#{slot}"));
    let active_candidates = vec![source_face("old", 10), source_face("new", 10)];
    assert_eq!(
        select_legacy_extrude_face_candidate(
            &active_candidates,
            &AsmHistoricalTopology::default(),
            &HashSet::new(),
            Some("old"),
        ),
        Some(LegacyFaceResolution::Active(source_face("old", 10)))
    );
    assert_eq!(
        select_legacy_extrude_face_candidate(
            &active_candidates,
            &AsmHistoricalTopology::default(),
            &HashSet::new(),
            Some("missing"),
        ),
        None
    );

    let historical_candidates = vec![FaceId("f3d:brep:entity#20".into()), source_face("new", 21)];
    let topology = AsmHistoricalTopology {
        faces: vec![20, 21],
        ..AsmHistoricalTopology::default()
    };
    let mut changed = HashSet::new();
    changed.insert(21);
    assert_eq!(
        select_legacy_extrude_face_candidate(
            &historical_candidates,
            &topology,
            &changed,
            Some("new"),
        ),
        Some(LegacyFaceResolution::Historical(21))
    );
    assert_eq!(
        select_legacy_extrude_face_candidate(
            &[FaceId("f3d:brep:entity#20".into())],
            &topology,
            &changed,
            None,
        ),
        Some(LegacyFaceResolution::Historical(20))
    );
}

#[test]
fn hole_face_selection_binds_to_the_feature_input_topology() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory,
    };
    use crate::records::{
        DesignEntitySelectionFaceCandidate, DesignHoleConstruction, DesignHoleFaceSelection,
        DesignParameterScope,
    };
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureInputTopology, HoleKind,
        Length, Termination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let feature_id = FeatureId("f3d:feature#42".into());
    let scope_id = "f3d:Design/BulkStream.dat:scope#42";
    let mut scope = DesignParameterScope::empty(scope_id, "Hole", 42);
    scope.history_state_id = Some(2);
    scope.previous_history_state_id = Some(1);
    scope.hole_construction = Some(DesignHoleConstruction {
        point_record_index: 55,
        point_record_byte_offset: 0,
        position: [0.0; 3],
        position_offset: 0,
        direction: [0.0, 0.0, 1.0],
        direction_offset: 0,
        point_parameters: [0.0; 2],
        point_parameter_offsets: [0, 0],
        reference_type: 0,
        reference_type_offset: 0,
        tangent_point_data: None,
        tangent_point_data_prefix: None,
        tangent_point_data_offset: None,
        input_record_indices: vec![55],
        input_record_offsets: vec![0],
        face_selection: Some(DesignHoleFaceSelection {
            record_index: 100,
            byte_offset: 0,
            class_tag: "333".into(),
            asset_id: "asset".into(),
            asset_id_offset: 0,
            context_id: "context".into(),
            context_id_offset: 0,
            identity_record_index: 103,
            identity_record_offset: 0,
            primary_identity: 18044,
            primary_identity_offset: 0,
            secondary_identity: None,
            secondary_identity_offset: None,
            curve_secondary_identity: None,
            curve_secondary_identity_offset: None,
            historical_face_candidates: vec![DesignEntitySelectionFaceCandidate {
                history_id: "f3d:asset/Breps.BlobParts/BREP.example.smbh:asm-delta-state#2".into(),
                historical_entity_kind: AsmHistoricalEntityKind::Pcurve,
                historical_entity_ref: 18044,
                historical_state_ids: vec![1],
                face_slot: 30,
            }],
            next_record_index: 104,
            next_byte_offset: 0,
        }),
    });
    let mut feature = Feature::new(
        feature_id.clone(),
        0,
        FeatureDefinition::Hole {
            profile: None,
            profile_filter: None,
            face: Some(FaceSelection::Native(scope_id.into())),
            position: Some(Point3::new(0.0, 0.0, 0.0)),
            direction: Some(Vector3::new(0.0, 0.0, 1.0)),
            placements: Vec::new(),
            kind: HoleKind::Simple,
            exit_kind: None,
            diameter: Some(Length(5.0)),
            extent: Some(Termination::Blind {
                length: Length(10.0),
            }),
            bottom: None,
            taper_angle: None,
            specification: None,
            allow_multi_profile_faces: None,
        },
    );
    feature.native_ref = Some(scope_id.into());
    let mut input_topologies = vec![FeatureInputTopology {
        id: crate::design::edge_resolve::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id.clone(),
        bodies: Vec::new(),
        faces: Vec::new(),
        edges: Vec::new(),
        vertices: Vec::new(),
        native_ref: None,
    }];
    let state = |state_id, transition| AsmDeltaState {
        id: format!("history:state#{state_id}"),
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
        record_table_complete: true,
        topology: Some(AsmHistoricalTopology::default()),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        stream_size: None,
        history_entry_count: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![
            state(
                2,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: Default::default(),
                    topology: Default::default(),
                }),
            ),
            state(1, None),
        ],
    };

    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        &[scope],
        &[],
        &[],
        &[],
        &[],
        &[history],
    );

    let FeatureDefinition::Hole {
        face:
            Some(FaceSelection::Historical {
                state,
                faces,
                native,
            }),
        ..
    } = &feature.definition
    else {
        panic!("Hole support face remains unresolved");
    };
    assert_eq!(native, scope_id);
    assert_eq!(
        state,
        &crate::design::edge_resolve::feature_input_topology_id(&feature_id, 1)
    );
    assert_eq!(faces.len(), 1);
    assert_eq!(&input_topologies[0].faces, faces);
}
