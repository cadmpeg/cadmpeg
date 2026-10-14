// SPDX-License-Identifier: Apache-2.0
use super::super::{
    edge_recipe_structure_with_context, face_recipe_nodes_with_context, FaceRecipeProgramKind,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
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
