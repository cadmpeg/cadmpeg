// SPDX-License-Identifier: Apache-2.0
use super::super::{
    edge_recipe_structure_with_context, edge_recipe_structure_within,
    face_recipe_nodes_with_context, FaceRecipeProgramKind, RECIPE_SIDE_SEARCH_WORK,
};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget,
};
use cadmpeg_core::CodecError;

const EDGE_RECIPE: [i32; 24] = [
    0, 0, 0, 0, 0, 0, 0, 1, -1, 2, 0, -1, 1, -1, 0, 1, 1, 4, 1, 1, 1, 4, 4, 4,
];

#[test]
fn edge_recipe_structure_refuses_recursion_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        edge_recipe_structure_with_context(&ctx, &EDGE_RECIPE),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RecursionDepth
                && failure.operation == "f3d recipe side recursion"
    ));
}

#[test]
fn face_recipe_nodes_split_at_each_node_marker() {
    let program = [0, -1, 4, -1, -1, 2, 7];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let nodes = face_recipe_nodes_with_context(
        &ctx,
        &program,
        0,
        FaceRecipeProgramKind::Counted { header_value: 4 },
    )
    .unwrap()
    .unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].program, [-1, -1, 2, 7]);
    assert_eq!(nodes[0].byte_offset, 12);
}

/// An edge-recipe program whose one counted side has 3,000 payload-prefix
/// splits, so the side search needs more work than its cap allows.
fn exhausting_edge_recipe_program() -> Vec<i32> {
    let mut program = vec![0; 7];
    program.extend_from_slice(&[1, -1, 2, 5, -1, 7, -1]);
    program.extend_from_slice(&[0; 3000]);
    program
}

#[test]
fn edge_recipe_side_search_skips_the_structure_past_its_work_cap() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let program = exhausting_edge_recipe_program();
    assert_eq!(
        edge_recipe_structure_with_context(&ctx, &program).unwrap(),
        None
    );
    let budget = WorkBudget::new(RECIPE_SIDE_SEARCH_WORK);
    assert_eq!(
        edge_recipe_structure_within(&ctx, &budget, &program).unwrap(),
        None
    );
    assert!(budget.exhausted());
    assert_eq!(ctx.resource_refusal(), None);

    // The same search on a resolvable program completes within the cap and
    // keeps its structure; a cap too small for it skips the structure.
    let budget = WorkBudget::new(RECIPE_SIDE_SEARCH_WORK);
    assert!(edge_recipe_structure_within(&ctx, &budget, &EDGE_RECIPE)
        .unwrap()
        .is_some());
    assert!(!budget.exhausted());
    let budget = WorkBudget::new(1);
    assert_eq!(
        edge_recipe_structure_within(&ctx, &budget, &EDGE_RECIPE).unwrap(),
        None
    );
    assert!(budget.exhausted());
}
