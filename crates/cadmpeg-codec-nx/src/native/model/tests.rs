use super::terminal_feature_body_ids;
use crate::native::segments::{SegmentBodyBinding, SegmentBodyLineageStatus};
use crate::parasolid::StreamKind;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::ids::BodyId;
use std::collections::BTreeSet;

fn one_terminal_body() -> (
    BTreeSet<BodyId>,
    [SegmentBodyBinding; 1],
    [SegmentBodyLineageStatus; 1],
) {
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "link#0".into(),
        stream_ordinal: 0,
        stream_kind: StreamKind::Partition,
        body_object_index: 10,
        body_alias_object_index: 20,
        stream_role: 0,
        source_offset: 0,
    };
    let status = SegmentBodyLineageStatus {
        id: "status#0".into(),
        segment_body_binding: binding.id.clone(),
        body_object_index: 10,
        body_alias_object_index: 20,
        terminal: true,
        source_offset: 0,
    };
    (
        BTreeSet::from([BodyId::mint("nx:s0:body#0").expect("valid body ID")]),
        [binding],
        [status],
    )
}

#[test]
fn terminal_body_selection_joins_statuses_by_binding_identity() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("empty root fits service policy");
    let bindings = [0, 1].map(|ordinal| SegmentBodyBinding {
        id: format!("binding#{ordinal}"),
        stream_link: format!("link#{ordinal}"),
        stream_ordinal: ordinal,
        stream_kind: StreamKind::Partition,
        body_object_index: 10 + ordinal,
        body_alias_object_index: 20 + ordinal,
        stream_role: 0,
        source_offset: u64::from(ordinal),
    });
    let status = |binding: &SegmentBodyBinding, terminal| SegmentBodyLineageStatus {
        id: format!("status#{}", binding.stream_ordinal),
        segment_body_binding: binding.id.clone(),
        body_object_index: binding.body_object_index,
        body_alias_object_index: binding.body_alias_object_index,
        terminal,
        source_offset: binding.source_offset,
    };
    let emitted = ["nx:s0:body#0", "nx:s1:body#0"]
        .map(|id| BodyId::mint(id).unwrap())
        .into_iter()
        .collect::<BTreeSet<_>>();
    let statuses = [status(&bindings[1], true), status(&bindings[0], false)];
    let selected = BTreeSet::from([BodyId::mint("nx:s1:body#0").unwrap()]);
    assert_eq!(
        terminal_feature_body_ids(&ctx, &emitted, &bindings, &statuses).unwrap(),
        Some(selected)
    );

    let mut mismatched = statuses.clone();
    mismatched[1].segment_body_binding = "missing".into();
    assert!(
        terminal_feature_body_ids(&ctx, &emitted, &bindings, &mismatched)
            .unwrap()
            .is_none()
    );
    let duplicate = [statuses[0].clone(), statuses[0].clone()];
    assert!(
        terminal_feature_body_ids(&ctx, &emitted, &bindings, &duplicate)
            .unwrap()
            .is_none()
    );
    assert!(
        terminal_feature_body_ids(&ctx, &emitted, &bindings, &statuses[..1])
            .unwrap()
            .is_none()
    );
    let mut extra = statuses.to_vec();
    let mut unmatched = statuses[0].clone();
    unmatched.segment_body_binding = "extra".into();
    extra.push(unmatched);
    assert!(terminal_feature_body_ids(&ctx, &emitted, &bindings, &extra)
        .unwrap()
        .is_none());
    let duplicate_bindings = [bindings[0].clone(), bindings[0].clone()];
    assert!(
        terminal_feature_body_ids(&ctx, &emitted, &duplicate_bindings, &statuses)
            .unwrap()
            .is_none()
    );
}

#[test]
fn terminal_body_selection_refuses_status_index_at_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits service policy");
    let status = SegmentBodyLineageStatus {
        id: "status#0".into(),
        segment_body_binding: "binding#0".into(),
        body_object_index: 10,
        body_alias_object_index: 20,
        terminal: true,
        source_offset: 0,
    };
    let error = terminal_feature_body_ids(&ctx, &BTreeSet::new(), &[], &[status])
        .expect_err("status index needs one collection item");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected a resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "nx terminal body status index");
}

#[test]
fn terminal_body_selection_refuses_mapped_set_at_collection_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = terminal_feature_body_ids(&ctx, &emitted, &bindings, &statuses)
        .expect_err("status index uses the only collection item");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx mapped terminal body"
    ));
}

#[test]
fn terminal_body_selection_refuses_selected_set_at_collection_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = terminal_feature_body_ids(&ctx, &emitted, &bindings, &statuses)
        .expect_err("status and mapped body use both collection items");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx selected terminal body"
    ));
}

#[test]
fn terminal_body_selection_refuses_prefix_materialization_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = terminal_feature_body_ids(&ctx, &emitted, &bindings, &statuses)
        .expect_err("six-byte prefix exceeds five temporary bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "nx terminal body prefix"
    ));
}

#[test]
fn terminal_body_selection_refuses_scan_work_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = terminal_feature_body_ids(&ctx, &emitted, &bindings, &statuses)
        .expect_err("one body scan exceeds zero work units");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "nx terminal body scan"
    ));
}

#[test]
fn terminal_body_selection_refuses_identity_retention_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let error = terminal_feature_body_ids(&ctx, &emitted, &bindings, &statuses)
        .expect_err("mapped identity cannot be retained");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "nx mapped terminal body identity"
    ));
}
