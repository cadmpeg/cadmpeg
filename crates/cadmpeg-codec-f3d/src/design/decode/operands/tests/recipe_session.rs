// SPDX-License-Identifier: Apache-2.0

use super::super::{
    edge_recipe_structure_with_context, face_recipe_structure_with_context,
    surface_patch_recipe_structure_with_context,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

#[test]
fn recipe_prefix_refusal_empty_and_fixed_invalid_grammars_keep_original_error() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
        policy.limits.max_work_units + 1,
        "original topology recipe prefix refusal",
    ) else {
        panic!("creating session refuses excess work");
    };
    for program in [&[][..], &[1][..], &[0; 7][..], &[0; 8][..]] {
        assert!(matches!(edge_recipe_structure_with_context(&ctx, program),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(
            matches!(surface_patch_recipe_structure_with_context(&ctx, program, usize::MAX),
            Err(CodecError::ResourceLimit(later)) if later == original)
        );
        assert!(matches!(face_recipe_structure_with_context(&ctx, program),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn recipe_prefix_refusal_empty_and_fixed_invalid_grammars_need_no_positive_resources() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for program in [&[][..], &[1][..], &[0; 7][..], &[0; 8][..]] {
        assert!(edge_recipe_structure_with_context(&ctx, program)
            .unwrap()
            .is_none());
        assert!(
            surface_patch_recipe_structure_with_context(&ctx, program, usize::MAX)
                .unwrap()
                .is_none()
        );
    }
    for program in [&[][..], &[1][..], &[0, 1][..]] {
        assert!(face_recipe_structure_with_context(&ctx, program)
            .unwrap()
            .is_none());
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}
