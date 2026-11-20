// SPDX-License-Identifier: Apache-2.0

#[test]
fn surface_patch_recipe_uses_the_unique_common_boundary_edge() {
    use crate::history_records::{
        AsmHistoricalCoedge, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use crate::records::{
        dimensions::DesignRecipeReference,
        topology::{
            edge_recipe::DesignSurfacePatchRecipeClause,
            edge_recipe::DesignSurfacePatchRecipeStructure,
        },
    };
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
        token_integer: 0,
        token_offset: 0,
        design_reference: 0,
        design_reference_offset: 0,
        candidate_faces,
        candidate_edges,
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
        operand_offset: None,
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
        crate::history::surface_patch_edge_operand_slot(
            Some(&structure),
            &references,
            &topology,
            &mut crate::history::topology_cache::TopologyQueryCache::default()
        ),
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
        crate::history::surface_patch_edge_operand_slot(
            Some(&structure),
            &references,
            &ambiguous,
            &mut crate::history::topology_cache::TopologyQueryCache::default()
        ),
        None
    );
}

#[test]
fn surface_patch_reference_roles_deduplicate_repeated_ordinals() {
    let common = crate::history::common_surface_patch_reference;
    assert_eq!(common([1, 1], [3, 1]), Some(1));
    assert_eq!(common([0, 2], [0, 4]), Some(0));
    assert_eq!(common([0, 0], [0, 0]), Some(0));
    assert_eq!(common([1, 2], [1, 2]), None);
    assert_eq!(common([1, 2], [3, 4]), None);
}
