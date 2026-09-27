// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use super::super::source_association;
use super::{
    commit_curve_tree, hatch_loop_ids, hatch_source_links, object_record, one_child_compound,
    scan_with_objects, stage_curve_tree, test_association, with_collection_limit,
    with_transaction_limits, ArchiveVersion, BrepDraft, CadIr, CandidateError, CurveCommitSource,
    DecodeContext, POINT_CLASS,
};

#[test]
fn class_loss_tag_refuses_retained_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let outcome = super::super::ClassOutcome {
        decoded: 0,
        retained: 1,
        native: None,
        attribute_degraded: 0,
        failed_framed: 0,
        first_object: &scan.objects[0],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let refusal = super::super::loss_provenance(&ctx, "fixture", &outcome)
        .expect_err("class loss tag exceeds retained limit");
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino class loss tag"
    ));
    super::super::loss_provenance(
        &cadmpeg_test_support::service_decode_context(),
        "fixture",
        &outcome,
    )
    .expect("service profile admits the tag");
}

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

#[test]
fn class_outcome_keys_refuse_collection_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let error = with_transaction_limits(&scan, 4, None, |expand| {
        let context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .class_outcomes()
            .expect_err("class key exceeds four collection items")
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.operation == "Rhino class outcome keys")
    );
    let outcomes = with_transaction_limits(&scan, 6, None, |expand| {
        let context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .class_outcomes()
            .expect("service-size class outcome")
            .len()
    });
    assert_eq!(outcomes, 1);
}

#[test]
fn class_outcome_rows_refuse_collection_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let error = with_transaction_limits(&scan, 5, None, |expand| {
        let context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .class_outcomes()
            .expect_err("class row exceeds five collection items")
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.operation == "Rhino class outcome rows")
    );
}

#[test]
fn class_outcome_label_refuses_retained_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let retained_record_bytes =
        u64::try_from(scan.objects[0].range().len()).expect("bounded point-cloud fixture");
    let error = with_transaction_limits(&scan, 6, Some(retained_record_bytes), |expand| {
        let context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .class_outcomes()
            .expect_err("class label exceeds retained record bytes")
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.operation == "Rhino class outcome label")
    );
}

#[test]
fn source_association_refuses_retained_copy_and_path_limits() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let identity = scan.objects[0].identity().expect("source identity");
    let refusal = with_transaction_limits(&scan, 1, Some(35), |expand| {
        source_association(expand.ctx(), identity, &[], None, None)
            .expect_err("UUID text exceeds 35 retained bytes")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino source association object ID"
    ));

    let mut named = identity.clone();
    named.name = "part".to_string();
    let refusal = with_transaction_limits(&scan, 1, Some(36), |expand| {
        source_association(expand.ctx(), &named, &[], None, None)
            .expect_err("name exceeds UUID-only retained budget")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino source association name"
    ));

    let mut layered = identity.clone();
    layered.layer = Some(crate::objects::LayerRef {
        id: None,
        name: "layer".to_string(),
    });
    let refusal = with_transaction_limits(&scan, 1, Some(36), |expand| {
        source_association(expand.ctx(), &layered, &[], None, None)
            .expect_err("layer name exceeds UUID-only retained budget")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino source association layer name"
    ));
    layered.layer.as_mut().expect("fixture layer").id = Some(identity.object_id);
    let refusal = with_transaction_limits(&scan, 1, Some(36), |expand| {
        source_association(expand.ctx(), &layered, &[], None, None)
            .expect_err("layer UUID exceeds object UUID-only retained budget")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino source association layer ID"
    ));

    let path = vec!["instance".to_string()];
    let refusal = with_transaction_limits(&scan, 0, None, |expand| {
        source_association(expand.ctx(), identity, &path, None, None)
            .expect_err("one instance path exceeds zero collection items")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino source association instance path"
    ));
    let refusal = with_transaction_limits(&scan, 1, Some(36), |expand| {
        source_association(expand.ctx(), identity, &path, None, None)
            .expect_err("instance ID exceeds object UUID-only retained budget")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(ref limit)
            if limit.operation == "Rhino source association instance ID"
    ));
    let association = with_transaction_limits(&scan, 1, None, |expand| {
        source_association(expand.ctx(), identity, &path, None, None)
            .expect("service profile admits source association")
    });
    assert_eq!(
        association.object_id.as_str(),
        identity.object_id.to_string()
    );
    assert_eq!(association.instance_path, path);
}
