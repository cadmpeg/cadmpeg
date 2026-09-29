// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::disallowed_methods)]

use super::super::source_association;
use super::{
    commit_curve_tree, hatch_loop_ids, hatch_source_links, line_nurbs, object_record,
    one_child_compound, scan_with_objects, stage_curve_tree, test_association,
    with_collection_limit, with_transaction_limits, ArchiveVersion, BrepDraft, CadIr,
    CandidateError, CurveCommitSource, CurveGeometry, DecodeContext, Diagnostics, RhinoLossCode,
    SolvedCurveGeometry, POINT_CLASS,
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

#[test]
fn full_source_attributes_refuse_collection_limit() {
    let mut scan = scan_with_objects(&[]);
    scan.metadata.settings.current_layer = Some(7);
    let refusal = with_collection_limit(0, |ctx| {
        super::super::full_source_attributes(ctx, &scan)
            .expect_err("one source attribute exceeds zero collection items")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino full source attributes"
    ));
    let attributes = super::super::full_source_attributes(
        &cadmpeg_test_support::service_decode_context(),
        &scan,
    )
    .expect("source attributes admitted by service profile");
    assert_eq!(attributes.get("current_layer"), Some(&"7".to_string()));
}

#[test]
fn feature_property_map_refuses_collection_limit() {
    let error = with_collection_limit(0, |ctx| {
        super::super::insert_feature_property(
            ctx,
            &mut std::collections::BTreeMap::new(),
            format_args!("dimension"),
            format_args!("3"),
        )
        .expect_err("one generated feature property exceeds zero collection items")
    });
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino feature property entries"
    ));
    let mut properties = std::collections::BTreeMap::new();
    super::super::insert_feature_property(
        &cadmpeg_test_support::service_decode_context(),
        &mut properties,
        format_args!("dimension"),
        format_args!("3"),
    )
    .expect("service profile admits the property");
    assert_eq!(properties["dimension"], "3");
}

#[test]
fn scan_warning_and_diagnostic_refuse_collection_limit() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let refusal = with_transaction_limits(&scan, 4, None, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .scan_warning(0, format_args!("decode failed"))
            .expect_err("warning requires a report slot")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino diagnostics"
    ));
    let diagnostic = crate::loss::RhinoDiagnostic {
        code: Some(RhinoLossCode::IntegrityFailure),
        message: "checksum mismatch".to_string(),
    };
    let refusal = with_transaction_limits(&scan, 4, None, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .scan_diagnostic(0, &diagnostic)
            .expect_err("coded diagnostic requires a report slot")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino diagnostics"
    ));
    with_transaction_limits(&scan, 6, None, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction admitted");
        context
            .scan_warning(0, format_args!("decode failed"))
            .expect("warning admitted");
        context
            .scan_diagnostic(0, &diagnostic)
            .expect("coded diagnostic admitted");
        assert_eq!(context.report.phase_warnings.len(), 2);
    });
}

#[test]
fn typed_report_loss_and_prefixed_diagnostic_refuse_collection_limit() {
    let refusal = with_collection_limit(0, |ctx| {
        super::super::push_report_loss(
            ctx,
            &mut Vec::new(),
            RhinoLossCode::DimensionOverrideDropped,
            format_args!("override dropped"),
        )
        .expect_err("typed loss requires a collection slot")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino typed decode losses"
    ));
    let mut source = Diagnostics::new();
    source.push("repaired");
    let refusal = with_collection_limit(0, |ctx| {
        Diagnostics::new()
            .append_prefixed_admitted(ctx, source, format_args!("object"))
            .expect_err("prefixed diagnostic requires a collection slot")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino diagnostics"
    ));
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut losses = Vec::new();
    super::super::push_report_loss(
        &ctx,
        &mut losses,
        RhinoLossCode::DimensionOverrideDropped,
        format_args!("override dropped"),
    )
    .expect("service profile admits the typed loss");
    assert_eq!(losses[0].message, "override dropped");
}

#[test]
fn curve_warning_tree_refuses_collection_limit() {
    let mut warnings = Diagnostics::new();
    warnings.push("curve repair");
    let curve = crate::curves::DecodedCurve::leaf(
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(line_nurbs(0.0, 1.0, false))),
        warnings,
    );
    let error = with_collection_limit(0, |ctx| {
        super::super::append_curve_warnings(ctx, &mut Diagnostics::new(), &curve, "source")
            .expect_err("curve diagnostic requires a collection slot")
    });
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino diagnostics"
    ));
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut report = Diagnostics::new();
    super::super::append_curve_warnings(&ctx, &mut report, &curve, "source")
        .expect("service profile admits the warning");
    assert_eq!(report[0].message, "source: curve repair");
}

#[test]
fn typed_install_loss_transfer_refuses_collection_limit() {
    let source = vec![RhinoLossCode::IntegrityFailure.note("invalid source record")];
    let refusal = with_collection_limit(0, |ctx| {
        super::super::append_report_losses(ctx, &mut Vec::new(), source)
            .expect_err("typed loss transfer requires one report slot")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino typed decode losses"
    ));
    let source = vec![RhinoLossCode::IntegrityFailure.note("invalid source record")];
    let mut report = Vec::new();
    super::super::append_report_losses(
        &cadmpeg_test_support::service_decode_context(),
        &mut report,
        source,
    )
    .expect("service profile admits the loss");
    assert_eq!(report[0].message, "invalid source record");
}

#[test]
fn instance_unique_members_refuse_collection_limit() {
    let members = [crate::wire::Uuid::from_canonical([0x51; 16])];
    let refusal = with_collection_limit(0, |ctx| {
        super::super::instance_members_are_unique(ctx, &members)
            .expect_err("one member requires one set node")
    });
    assert!(matches!(
        refusal,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "Rhino instance unique members"
    ));
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(super::super::instance_members_are_unique(&ctx, &members).expect("one unique member"));
    assert!(
        !super::super::instance_members_are_unique(&ctx, &[members[0], members[0]])
            .expect("duplicate members are admitted before rejection")
    );
}
