// SPDX-License-Identifier: Apache-2.0
use crate::history::face_selections::bind_feature_face_selections;
use crate::history_records::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalCarrierBinding, AsmHistoricalPlane,
    AsmHistoricalTopology, AsmHistory,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::evaluation::{FeatureEvaluation, FeatureInputTopology};
use cadmpeg_ir::features::operation_settings::ShellMode;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};

const SCOPE_ID: &str = "f3d:Design/BulkStream.dat:scope#42";
const GROUP_ID: &str = "f3d:Design/BulkStream.dat:group#43";

/// Faces 1 to 6 of a 40 x 30 x 20 mm box with the top as face 2, and the
/// inner walls 11 and 13 to 16 that a 2 mm inside shell adds to the others.
fn topology(walled: bool) -> AsmHistoricalTopology {
    let mut faces = vec![
        (1, Point3::new(20.0, 15.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
        (
            2,
            Point3::new(20.0, 15.0, 20.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        (3, Point3::new(0.0, 30.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)),
        (4, Point3::new(40.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (5, Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, -1.0, 0.0)),
        (6, Point3::new(40.0, 30.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
    ];
    if walled {
        faces.extend([
            (
                11,
                Point3::new(20.0, 15.0, 2.0),
                Vector3::new(0.0, 0.0, -1.0),
            ),
            (13, Point3::new(2.0, 30.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
            (
                14,
                Point3::new(38.0, 0.0, 0.0),
                Vector3::new(-1.0, 0.0, 0.0),
            ),
            (15, Point3::new(0.0, 2.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
            (
                16,
                Point3::new(40.0, 28.0, 0.0),
                Vector3::new(0.0, -1.0, 0.0),
            ),
        ]);
    }
    AsmHistoricalTopology {
        faces: faces.iter().map(|(face, ..)| *face).collect(),
        face_surfaces: faces
            .iter()
            .map(|(face, ..)| AsmHistoricalCarrierBinding {
                entity: *face,
                carrier: 100 + face,
            })
            .collect(),
        surface_planes: faces
            .iter()
            .map(|(face, origin, normal)| AsmHistoricalPlane {
                surface: 100 + face,
                origin: *origin,
                normal: *normal,
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    }
}

/// A state whose faces in `revised` carry a second record revision.
fn state(
    state_id: i64,
    topology: AsmHistoricalTopology,
    next_ref: Option<i64>,
    revised: &[i64],
) -> crate::test_support::history::State {
    let versions = topology
        .faces
        .iter()
        .map(|&face| AsmEntityVersion {
            entity_ref: face,
            record_ref: if revised.contains(&face) { 2 } else { 1 },
        })
        .collect();
    (
        AsmDeltaState {
            id: format!("history:state#{state_id}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(topology),
        versions,
    )
}

fn history(updated: &[i64]) -> AsmHistory {
    crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(2, topology(true), Some(1), updated),
            state(1, topology(false), None, &[]),
        ],
    )
}

fn scope(record_index: u32) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(SCOPE_ID, DesignFeatureKind::Shell, record_index);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn group(scope_record_index: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    group_in(GROUP_ID, scope_record_index, members)
}

fn group_in(id: &str, scope_record_index: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    let member_offsets: Vec<u64> = (0..u64::try_from(members.len()).unwrap())
        .map(|ordinal| ordinal * 11)
        .collect();
    json::from_value(serde_json::json!({
        "id": id,
        "byte_offset": 0,
        "class_tag": "261",
        "record_index": 43,
        "scope_record_index": scope_record_index,
        "scope_reference_ordinal": 0,
        "members": members,
        "member_offsets": member_offsets,
        "role": 68_719_476_736_u64,
        "role_offset": 0,
        "paired_byte_offset": 200,
        "paired_class_tag": "266",
        "frame": {
            "member_count_offset": 0,
            "opaque_index": 1,
            "opaque_index_offset": 18,
            "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22,
            "trailing_flags": [],
            "trailing_record_indices": [],
            "trailing_record_offsets": [],
            "variant": false
        }
    }))
    .unwrap()
}

fn bind(
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    mode: Option<ShellMode>,
    updated: &[i64],
) -> FaceSelection {
    bind_group(GROUP_ID, scope, groups, mode, updated)
}

fn bind_group(
    group_id: &str,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    mode: Option<ShellMode>,
    updated: &[i64],
) -> FaceSelection {
    let feature_id = FeatureId::mint("f3d:test:feature#42").expect("identity grammar");
    let mut feature = Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Shell {
                bodies: None,
                removed_faces: FaceSelection::Native(group_id.into()),
                thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap()),
                outward: Some(false),
                mode,
                join: None,
                resolve_intersections: None,
                allow_self_intersections: None,
            },
        )),
        native_ref: Some(SCOPE_ID.into()),
    };
    let mut input_topologies = vec![FeatureInputTopology {
        face_owners: Vec::new(),
        face_geometry: Vec::new(),
        body_writers: Vec::new(),
        id: crate::ids::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id,
        bodies: cadmpeg_ir::features::selections::DistinctMembers::try_from(Vec::new()).unwrap(),
        faces: cadmpeg_ir::features::selections::DistinctMembers::try_from(Vec::new()).unwrap(),
        edges: cadmpeg_ir::features::selections::DistinctMembers::try_from(Vec::new()).unwrap(),
        vertices: cadmpeg_ir::features::selections::DistinctMembers::try_from(Vec::new()).unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];
    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        crate::history::face_selections::FeatureFaceSelectionInputs {
            scopes: std::slice::from_ref(scope),
            groups,
            operands: &[],
            entity_operands: &[],
            body_recipe_operands: &[],
            histories: &mut [history(updated)],
            scope_histories: &std::collections::HashMap::new(),
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::Shell { removed_faces, .. }) =
        feature.evaluation.definition()
    else {
        panic!("expected a Shell");
    };
    removed_faces.clone()
}

fn selected_slots(selection: &FaceSelection) -> Vec<String> {
    let FaceSelection::Historical { faces, .. } = selection else {
        panic!("expected a historical selection, got {selection:?}");
    };
    faces
        .iter()
        .map(|face| face.as_str().rsplit(':').next().unwrap().to_owned())
        .collect()
}

const TOP: &[i64] = &[2];
const ALL: &[i64] = &[1, 2, 3, 4, 5, 6];

#[test]
fn shell_recipe_reads_tags_before_later_face_deletion() {
    use crate::history_records::AsmHistoricalPersistentSubentityTag;
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
    use crate::test_support::history::{bounded_face_operand, recipe_operand_reference};

    let mut input = topology(false);
    input
        .persistent_subentity_tags
        .push(AsmHistoricalPersistentSubentityTag {
            entity_kind: AsmHistoricalEntityKind::Face,
            entity_ref: 2,
            selector: 1,
            token: "7".into(),
            token_integer: 0,
            design_references: vec![113],
            ordinal: 0,
        });
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(3, AsmHistoricalTopology::default(), Some(2), &[]),
            state(2, topology(true), Some(1), TOP),
            state(1, input, None, &[]),
        ],
    );
    let mut operand = bounded_face_operand(vec![recipe_operand_reference(0, "7", 113, &[])]);
    operand.recipe_kind = crate::records::recipes::ConstructionRecipeKind::Face;
    operand.group.as_mut().unwrap().group_record_index = 43;
    assert!(operand.candidate_faces.is_empty());
    let mut operands = vec![operand];
    crate::test_support::with_decode_context(|ctx| {
        crate::history::face_operand_history::bind_face_operand_history_candidates(
            ctx,
            &mut operands,
            &[scope(42)],
            &[group(42, &[110])],
            &[],
            crate::history::binding_histories::BindingHistories::Shared(std::slice::from_ref(
                &history,
            )),
            &std::collections::HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(operands[0].resolved_face_slots, [2]);
    assert_eq!(
        operands[0].preceding_candidate_faces,
        [crate::ids::brep_face_id(2)]
    );
    assert!(history
        .topology(&history.states()[0])
        .unwrap()
        .faces
        .is_empty());
}

#[test]
fn shell_group_binds_to_the_face_its_transition_opens() {
    for updated in [TOP, ALL] {
        assert_eq!(
            selected_slots(&bind(&scope(42), &[group(42, &[7])], None, updated)),
            ["2"]
        );
    }
}

#[test]
fn a_shell_on_both_sides_walls_at_half_its_thickness() {
    assert_eq!(
        bind(
            &scope(42),
            &[group(42, &[7])],
            Some(ShellMode::BothSides),
            ALL
        ),
        FaceSelection::Native(GROUP_ID.into()),
        "each side of a 2 mm shell on both sides is 1 mm, and the walls are 2 mm away"
    );
}

#[test]
fn a_skin_shell_states_no_selection() {
    assert_eq!(
        bind(&scope(42), &[group(42, &[7])], Some(ShellMode::Skin), TOP),
        FaceSelection::Native(GROUP_ID.into())
    );
}

#[test]
fn shell_group_of_another_scope_does_not_bind() {
    assert_eq!(
        bind(&scope(42), &[group(41, &[7])], None, TOP),
        FaceSelection::Native(GROUP_ID.into())
    );
}

#[test]
fn shell_group_id_must_name_one_group() {
    assert_eq!(
        bind(&scope(42), &[group(42, &[7]), group(42, &[7])], None, TOP),
        FaceSelection::Native(GROUP_ID.into())
    );
}

#[test]
fn shell_group_with_more_members_than_open_faces_does_not_bind() {
    assert_eq!(
        bind(&scope(42), &[group(42, &[7, 8])], None, TOP),
        FaceSelection::Native(GROUP_ID.into())
    );
}

#[test]
fn shell_group_of_another_stream_does_not_bind() {
    const OTHER: &str = "f3d:Design/Other.dat:group#43";
    assert_eq!(
        bind_group(OTHER, &scope(42), &[group_in(OTHER, 42, &[7])], None, TOP),
        FaceSelection::Native(OTHER.into())
    );
}

fn bounded_shell_input() -> (
    AsmHistoricalTopology,
    Vec<crate::records::dimensions::DesignRecipeReference>,
) {
    use crate::history_records::{
        AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalPersistentSubentityTag,
        AsmHistoricalRelation,
    };
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
    use crate::test_support::history::recipe_operand_reference;

    let mut input = AsmHistoricalTopology {
        faces: vec![100, 101, 102, 103],
        edges: (10..16).collect(),
        vertices: (1..5).collect(),
        edge_vertices: [
            (10, 1, 2),
            (11, 2, 3),
            (12, 3, 1),
            (13, 1, 4),
            (14, 4, 2),
            (15, 3, 4),
        ]
        .into_iter()
        .map(|(edge, start_vertex, end_vertex)| AsmHistoricalEdge {
            edge,
            start_vertex,
            end_vertex,
        })
        .collect(),
        ..Default::default()
    };
    let mut references = Vec::new();
    for (ordinal, edges) in [[10, 11, 12], [10, 13, 14], [11, 14, 15], [12, 15, 13]]
        .into_iter()
        .enumerate()
    {
        let ordinal = i64::try_from(ordinal).unwrap();
        let face = 100 + ordinal;
        let loop_ = face + 100;
        let coedges: Vec<_> = (face * 10..face * 10 + 3).collect();
        input.loops.push(loop_);
        input.coedges.extend(&coedges);
        input.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![loop_],
        });
        input.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: coedges.clone(),
        });
        for (position, edge) in edges.into_iter().enumerate() {
            input.coedge_topology.push(AsmHistoricalCoedge {
                coedge: coedges[position],
                owner_loop: loop_,
                edge,
                next: coedges[(position + 1) % 3],
                previous: coedges[(position + 2) % 3],
                radial_next: 0,
            });
        }
        let token = (ordinal + 1).to_string();
        references.push(recipe_operand_reference(
            u64::try_from(ordinal).unwrap(),
            &token,
            10 + ordinal,
            &[],
        ));
        input
            .persistent_subentity_tags
            .push(AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref: face,
                selector: 1,
                token,
                token_integer: 0,
                design_references: vec![10 + ordinal],
                ordinal: 0,
            });
    }
    (input, references)
}

fn bounded_shell_member(
    references: Vec<crate::records::dimensions::DesignRecipeReference>,
    unread_node: bool,
) -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::topology::face::DesignFaceRecipeNode;
    use crate::test_support::history::bounded_face_operand;

    let mut operand = bounded_face_operand(references);
    operand.group.as_mut().unwrap().group_record_index = 43;
    operand.candidate_faces = vec![crate::ids::brep_face_id(101)];
    operand.recipe_program = vec![0, -1, 3];
    for (other, fans) in [
        (1, [[0, 1, 3], [0, 1, 2]]),
        (2, [[0, 1, 2], [0, 2, 3]]),
        (3, [[0, 2, 3], [0, 1, 3]]),
    ] {
        let mut fields = vec![0, -1, other, -1, 2, -1];
        for fan in fans {
            fields.extend([3, fan[0], -1, fan[1], -1, fan[2], -1, 0, 0, -1]);
        }
        let structure = crate::test_support::with_decode_context(|ctx| {
            crate::design::decode::operands::recipe_structure::face_recipe_structure(ctx, &fields)
                .unwrap()
                .unwrap()
        });
        let mut program = vec![-1, -1, 2];
        program.extend(fields);
        operand.recipe_program.extend(&program);
        operand.recipe_nodes.push(DesignFaceRecipeNode {
            byte_offset: 0,
            end_byte_offset: 12,
            program,
            recipe_structure: Some(structure),
        });
    }
    if unread_node {
        operand.recipe_nodes.last_mut().unwrap().recipe_structure = None;
    }
    operand
}

fn add_shell_inner_loop(input: &mut AsmHistoricalTopology) {
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation};

    input.faces.push(104);
    let mut tag = input
        .persistent_subentity_tags
        .iter()
        .find(|tag| tag.entity_ref == 100)
        .unwrap()
        .clone();
    tag.entity_ref = 104;
    input.persistent_subentity_tags.push(tag);
    input.edges.push(20);
    input.vertices.push(5);
    input.edge_vertices.push(AsmHistoricalEdge {
        edge: 20,
        start_vertex: 5,
        end_vertex: 5,
    });
    input
        .face_loops
        .iter_mut()
        .find(|row| row.owner_ref == 100)
        .unwrap()
        .member_refs
        .push(300);
    input.face_loops.push(AsmHistoricalRelation {
        owner_ref: 104,
        member_refs: vec![301],
    });
    for (loop_, coedge, radial_next) in [(300, 2000, 2001), (301, 2001, 2000)] {
        input.loops.push(loop_);
        input.coedges.push(coedge);
        input.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: vec![coedge],
        });
        input.coedge_topology.push(AsmHistoricalCoedge {
            coedge,
            owner_loop: loop_,
            edge: 20,
            next: coedge,
            previous: coedge,
            radial_next,
        });
    }
}

fn bind_bounded_shell_operand(
    unread_node: bool,
    inner_loop: bool,
) -> crate::records::topology::face::DesignFaceOperand {
    bind_bounded_shell_operand_with_aliases(unread_node, inner_loop, &[])
}

fn alias_shell_neighbors(input: &mut AsmHistoricalTopology, aliases: &[(i64, i64)]) {
    for &(left, right) in aliases {
        let tags = input
            .persistent_subentity_tags
            .iter()
            .filter(|tag| tag.entity_ref == left || tag.entity_ref == right)
            .cloned()
            .collect::<Vec<_>>();
        for mut tag in tags {
            tag.entity_ref = if tag.entity_ref == left { right } else { left };
            tag.ordinal = input
                .persistent_subentity_tags
                .iter()
                .filter(|existing| existing.entity_ref == tag.entity_ref)
                .map(|existing| existing.ordinal)
                .max()
                .unwrap()
                .checked_add(1)
                .unwrap();
            input.persistent_subentity_tags.push(tag);
        }
    }
    input
        .persistent_subentity_tags
        .sort_by_key(|tag| (tag.entity_ref, tag.ordinal));
}

fn bind_bounded_shell_operand_with_aliases(
    unread_node: bool,
    inner_loop: bool,
    aliases: &[(i64, i64)],
) -> crate::records::topology::face::DesignFaceOperand {
    let (mut input, references) = bounded_shell_input();
    alias_shell_neighbors(&mut input, aliases);
    if inner_loop {
        add_shell_inner_loop(&mut input);
    }
    let operand = bounded_shell_member(references, unread_node);
    let removed_loops = input
        .face_loops
        .iter()
        .find(|row| row.owner_ref == 100)
        .unwrap()
        .member_refs
        .clone();
    let removed_coedges = input
        .coedge_topology
        .iter()
        .filter(|row| removed_loops.contains(&row.owner_loop))
        .map(|row| row.coedge)
        .collect::<Vec<_>>();
    let mut output = input.clone();
    output.faces.retain(|face| *face != 100);
    output.loops.retain(|loop_| !removed_loops.contains(loop_));
    output
        .coedges
        .retain(|coedge| !removed_coedges.contains(coedge));
    output.face_loops.retain(|row| row.owner_ref != 100);
    output
        .loop_coedges
        .retain(|row| !removed_loops.contains(&row.owner_ref));
    output
        .coedge_topology
        .retain(|row| !removed_loops.contains(&row.owner_loop));
    output
        .persistent_subentity_tags
        .retain(|tag| tag.entity_ref != 100);
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(3, AsmHistoricalTopology::default(), Some(2), &[]),
            state(2, output, Some(1), &[]),
            state(1, input, None, &[]),
        ],
    );
    let mut operands = vec![operand];
    crate::test_support::with_decode_context(|ctx| {
        crate::history::face_operand_history::bind_face_operand_history_candidates(
            ctx,
            &mut operands,
            &[scope(42)],
            &[group(42, &[110])],
            &[],
            crate::history::binding_histories::BindingHistories::Shared(std::slice::from_ref(
                &history,
            )),
            &std::collections::HashMap::new(),
        )
    })
    .unwrap();
    operands.pop().unwrap()
}

#[test]
fn shell_boundary_program_resolves_the_preceding_face() {
    let operand = bind_bounded_shell_operand(false, false);
    assert_eq!(
        operand.preceding_candidate_faces,
        [crate::ids::brep_face_id(101)]
    );
    assert_eq!(operand.resolved_face_slots, [100]);
}

#[test]
fn unread_shell_boundary_keeps_member_unresolved() {
    let operand = bind_bounded_shell_operand(true, false);
    assert_eq!(
        operand.preceding_candidate_faces,
        [crate::ids::brep_face_id(101)]
    );
    assert!(operand.resolved_face_slots.is_empty());
}

#[test]
fn shell_boundary_loop_selects_the_face_with_an_inner_loop() {
    let operand = bind_bounded_shell_operand(false, true);
    assert_eq!(operand.resolved_face_slots, [100]);
}

#[test]
fn shell_boundary_program_assigns_edges_across_aliased_neighbors() {
    let operand = bind_bounded_shell_operand_with_aliases(false, false, &[(101, 102)]);
    assert_eq!(operand.resolved_face_slots, [100]);
}
