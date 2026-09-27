// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use super::{
    commit_curve_tree, hatch_loop_ids, hatch_source_links, one_child_compound, stage_curve_tree,
    test_association, with_collection_limit, BrepDraft, CadIr, CandidateError, CurveCommitSource,
    DecodeContext,
};

fn staged_curve_tree_refusal(limit: u64, operation: &str) {
    let refusal = with_collection_limit(limit, |ctx| {
        let mut staged = BrepDraft::default();
        stage_curve_tree(
            ctx,
            &mut staged,
            one_child_compound(),
            "compound",
            "root",
            &test_association(),
            &DecodeContext::mint_unknown_id(0),
        )
        .expect_err("curve tree exceeds collection limit")
    });
    assert!(matches!(
        refusal,
        crate::curves::GeometryError::Codec(cadmpeg_core::CodecError::ResourceLimit(ref limit))
            if limit.operation == operation
    ));
}

fn committed_curve_tree_refusal(limit: u64, operation: &str) {
    let refusal = with_collection_limit(limit, |ctx| {
        commit_curve_tree(
            ctx,
            &mut CadIr::empty(),
            &mut cadmpeg_ir::Annotations::default(),
            one_child_compound(),
            CurveCommitSource {
                key: "compound",
                association: &test_association(),
                record: None,
                path: "root",
            },
        )
        .expect_err("curve tree exceeds collection limit")
    });
    assert!(matches!(
        refusal,
        CandidateError::Codec(cadmpeg_core::CodecError::ResourceLimit(ref limit))
            if limit.operation == operation
    ));
}

#[test]
fn staged_curve_tree_parameters_refuse_collection_limit() {
    staged_curve_tree_refusal(1, "Rhino Brep curve tree parameters");
}

#[test]
fn staged_curve_tree_components_refuse_collection_limit() {
    staged_curve_tree_refusal(2, "Rhino Brep curve tree components");
}

#[test]
fn committed_curve_tree_parameters_refuse_collection_limit() {
    committed_curve_tree_refusal(1, "Rhino committed curve tree parameters");
}

#[test]
fn committed_curve_tree_components_refuse_collection_limit() {
    committed_curve_tree_refusal(2, "Rhino committed curve tree components");
}

#[test]
fn hatch_loop_ids_refuse_collection_limit() {
    let error = with_collection_limit(0, |ctx| {
        hatch_loop_ids(
            ctx,
            "fixture",
            std::iter::once(crate::hatch::LoopKind::Outer),
        )
        .expect_err("one loop exceeds zero collection items")
    });
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino hatch loop IDs"
    ));
}

#[test]
fn hatch_loop_id_text_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let expected_len = "rhino:object:curve#fixture.hatch-loop-0".len();
    policy.limits.max_retained_bytes = u64::try_from(expected_len - 1).expect("bounded fixture");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = hatch_loop_ids(
        &ctx,
        "fixture",
        std::iter::once(crate::hatch::LoopKind::Outer),
    )
    .expect_err("the loop ID exceeds the retained-byte limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino hatch loop ID text"
    ));
}

#[test]
fn hatch_source_links_refuse_collection_limit() {
    let feature_id = cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "hatch", "feature"),
        cadmpeg_ir::identity_key!("fixture"),
    );
    let error = with_collection_limit(1, |ctx| {
        hatch_source_links(
            ctx,
            vec![(crate::hatch::LoopKind::Outer, "loop".to_string())],
            &feature_id,
        )
        .expect_err("two links exceed one collection item")
    });
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino hatch source links"
    ));
}

#[test]
fn hatch_feature_link_text_refuses_retained_limit() {
    let feature_id = cadmpeg_ir::features::FeatureId::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "hatch", "feature"),
        cadmpeg_ir::identity_key!("fixture"),
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from(feature_id.as_str().len() - 1).expect("bounded fixture");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = hatch_source_links(&ctx, Vec::new(), &feature_id)
        .expect_err("feature link exceeds retained-byte limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino hatch feature link text"
    ));
}
