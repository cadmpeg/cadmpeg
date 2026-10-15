// SPDX-License-Identifier: Apache-2.0
use super::{group, recipe_edge_operand, recipe_reference};
use crate::design::edge_resolve::{
    resolved_surface_patch_edge_group, surface_patch_grouped_recipe_edges, SurfacePatchRecipeEdges,
};
use cadmpeg_ir::ids::EdgeId;

#[test]
fn grouped_surface_patch_recipe_requires_agreeing_exact_references() {
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.recipe_references = vec![
        recipe_reference(&[]),
        recipe_reference(&[17]),
        recipe_reference(&[17]),
    ];
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.recipe_references = vec![recipe_reference(&[18]), recipe_reference(&[18])];

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| surface_patch_grouped_recipe_edges(
            &[&first, &second],
            decode_ctx
        ))
        .unwrap(),
        SurfacePatchRecipeEdges::Resolved(vec![
            EdgeId::mint("f3d:test:edge#17").expect("identity grammar"),
            EdgeId::mint("f3d:test:edge#18").expect("identity grammar")
        ])
    );
}

#[test]
fn grouped_surface_patch_recipe_rejects_ambiguous_or_repeated_edges() {
    let mut ambiguous = recipe_edge_operand(10, &[], &[]);
    ambiguous.recipe_references = vec![recipe_reference(&[17, 18])];
    let mut contradictory = recipe_edge_operand(11, &[], &[]);
    contradictory.recipe_references = vec![recipe_reference(&[18]), recipe_reference(&[19])];
    let absent = recipe_edge_operand(12, &[], &[]);
    let mut repeated = recipe_edge_operand(11, &[], &[]);
    repeated.recipe_references = vec![recipe_reference(&[17])];

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| surface_patch_grouped_recipe_edges(
            &[&ambiguous],
            decode_ctx
        ))
        .unwrap(),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| surface_patch_grouped_recipe_edges(
            &[&contradictory],
            decode_ctx
        ))
        .unwrap(),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| surface_patch_grouped_recipe_edges(
            &[&absent, &contradictory],
            decode_ctx
        ))
        .unwrap(),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| surface_patch_grouped_recipe_edges(
            &[&repeated, &repeated],
            decode_ctx
        ))
        .unwrap(),
        SurfacePatchRecipeEdges::Inconclusive
    );
}

#[test]
fn grouped_surface_patch_recipe_projects_historical_edges() {
    let mut group = group(2, 10);
    group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.recipe_references = vec![recipe_reference(&[17])];
    first.surface_patch_recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure {
            clauses: std::array::from_fn(|_| {
                crate::records::topology::edge_recipe::DesignSurfacePatchRecipeClause {
                    fields: vec![
                        vec![0],
                        vec![0],
                        vec![2, 0],
                        vec![0, 0],
                        vec![0],
                        vec![0, 0],
                    ],
                    face_reference_ordinals: [0, 0],
                    edge_reference_ordinals: [0, 0],

                    entries: Vec::new(),
                }
            }),
        },
    );
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.recipe_references = vec![recipe_reference(&[18])];
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#surface-patch")
        .expect("identity grammar");

    let selection = crate::test_support::with_decode_context(|decode_ctx| {
        resolved_surface_patch_edge_group(
            &group,
            std::slice::from_ref(&group),
            &[first, second],
            &[],
            Some(7),
            &feature_id,
            decode_ctx,
        )
    })
    .unwrap();
    let prefix = crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("surface-patch"), 7);
    assert!(matches!(
        selection,
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.as_slice() == [
                crate::ids::history_input_edge_id(&prefix, 17),
                crate::ids::history_input_edge_id(&prefix, 18),
            ]
    ));
}

#[test]
fn contradictory_surface_patch_references_suppress_generic_resolution() {
    let group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_references = vec![recipe_reference(&[17]), recipe_reference(&[18])];
    operand.resolved_edge_slot = Some(17);
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 1,
            sides: Vec::new(),
        },
    );
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#surface-patch")
        .expect("identity grammar");

    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| resolved_surface_patch_edge_group(
            &group,
            std::slice::from_ref(&group),
            std::slice::from_ref(&operand),
            &[],
            Some(7),
            &feature_id,
            decode_ctx
        ))
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}
