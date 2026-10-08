use super::terminal_feature_body_ids;
use crate::native::segments::{SegmentBodyBinding, SegmentBodyLineageStatus};
use crate::parasolid::StreamKind;
use cadmpeg_core::decode::ResourceDimension;
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
    crate::test_support::with_decode_context(|ctx| {
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
            terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses).unwrap(),
            Some(selected)
        );

        let mut mismatched = statuses.clone();
        mismatched[1].segment_body_binding = "missing".into();
        assert!(
            terminal_feature_body_ids(ctx, &emitted, &bindings, &mismatched)
                .unwrap()
                .is_none()
        );
        let duplicate = [statuses[0].clone(), statuses[0].clone()];
        assert!(
            terminal_feature_body_ids(ctx, &emitted, &bindings, &duplicate)
                .unwrap()
                .is_none()
        );
        assert!(
            terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses[..1])
                .unwrap()
                .is_none()
        );
        let mut extra = statuses.to_vec();
        let mut unmatched = statuses[0].clone();
        unmatched.segment_body_binding = "extra".into();
        extra.push(unmatched);
        assert!(terminal_feature_body_ids(ctx, &emitted, &bindings, &extra)
            .unwrap()
            .is_none());
        let duplicate_bindings = [bindings[0].clone(), bindings[0].clone()];
        assert!(
            terminal_feature_body_ids(ctx, &emitted, &duplicate_bindings, &statuses)
                .unwrap()
                .is_none()
        );
    });
}

#[test]
fn terminal_body_selection_refuses_status_index_at_collection_limit() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let status = SegmentBodyLineageStatus {
                id: "status#0".into(),
                segment_body_binding: "binding#0".into(),
                body_object_index: 10,
                body_alias_object_index: 20,
                terminal: true,
                source_offset: 0,
            };
            let error = terminal_feature_body_ids(ctx, &BTreeSet::new(), &[], &[status])
                .expect_err("status index needs one collection item");
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("expected a resource refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
            assert_eq!(limit.operation, "nx terminal body status index");
        },
    );
}

#[test]
fn terminal_body_selection_refuses_mapped_set_at_collection_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 1;
        },
        |ctx| {
            let error = terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses)
                .expect_err("status index uses the only collection item");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "nx mapped terminal body"
            ));
        },
    );
}

#[test]
fn terminal_body_selection_refuses_selected_set_at_collection_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 2;
        },
        |ctx| {
            let error = terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses)
                .expect_err("status and mapped body use both collection items");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == "nx selected terminal body"
            ));
        },
    );
}

#[test]
fn terminal_body_selection_refuses_prefix_materialization_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            let status_node = 11 * std::mem::size_of::<(&str, &SegmentBodyLineageStatus)>()
                + 16 * std::mem::size_of::<usize>()
                + 2 * std::mem::align_of::<usize>();
            policy.limits.max_materialized_bytes =
                cadmpeg_core::decode::u64_from_index(status_node) + 5;
        },
        |ctx| {
            let error = terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses)
                .expect_err("six-byte prefix exceeds five temporary bytes");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::MaterializedBytes
                        && limit.operation == "nx terminal body prefix"
            ));
        },
    );
}

#[test]
fn terminal_body_selection_refuses_scan_work_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();
    // Admit status-key lookup before refusing the body scan.
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx terminal body scan",
        |ctx| terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "nx terminal body scan")
    );
}

#[test]
fn terminal_body_selection_refuses_identity_retention_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        |ctx| {
            let error = terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses)
                .expect_err("selected identity cannot be retained");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == "nx selected terminal body identity"
            ));
        },
    );
}

#[test]
fn terminal_body_selection_refuses_status_iteration_work_limit() {
    let (emitted, bindings, statuses) = one_terminal_body();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            let error = terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses).unwrap_err();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("status admission must propagate a resource refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "nx terminal body statuses");
            assert_eq!(limit.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        },
    );
}

#[test]
fn terminal_body_selection_stops_at_duplicate_status_before_suffix_work() {
    let key = "binding#duplicate";
    let statuses = [
        SegmentBodyLineageStatus {
            id: "status#0".into(),
            segment_body_binding: key.into(),
            body_object_index: 10,
            body_alias_object_index: 20,
            terminal: true,
            source_offset: 0,
        },
        SegmentBodyLineageStatus {
            id: "status#1".into(),
            segment_body_binding: key.into(),
            body_object_index: 11,
            body_alias_object_index: 21,
            terminal: true,
            source_offset: 1,
        },
        SegmentBodyLineageStatus {
            id: "status#2".into(),
            segment_body_binding: "binding#suffix".into(),
            body_object_index: 12,
            body_alias_object_index: 22,
            terminal: true,
            source_offset: 2,
        },
    ];
    let first = statuses[0].clone();
    let refusal = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx terminal body status index",
        |ctx| terminal_feature_body_ids(ctx, &BTreeSet::new(), &[], std::slice::from_ref(&first)),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = refusal else {
        panic!("the first status insertion must have a work refusal boundary");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "nx terminal body status index");
    let key_len = u64::try_from(key.len()).expect("fixture length fits u64");
    // The one-row probe admits its insertion; the duplicate adds one visit and
    // one key comparison. The third status remains outside the admitted prefix.
    let work = limit.used + limit.additional + 1 + key_len;
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = work,
        |ctx| {
            assert!(terminal_feature_body_ids(ctx, &BTreeSet::new(), &[], &statuses)
                .unwrap()
                .is_none());
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn terminal_body_selection_returns_first_missing_binding_before_suffix_work() {
    let bindings = [
        SegmentBodyBinding {
            id: "binding#missing".into(),
            stream_link: "link#0".into(),
            stream_ordinal: 0,
            stream_kind: StreamKind::Partition,
            body_object_index: 10,
            body_alias_object_index: 20,
            stream_role: 0,
            source_offset: 0,
        },
        SegmentBodyBinding {
            id: "binding#suffix".into(),
            stream_link: "link#1".into(),
            stream_ordinal: 1,
            stream_kind: StreamKind::Partition,
            body_object_index: 11,
            body_alias_object_index: 21,
            stream_role: 0,
            source_offset: 1,
        },
    ];
    let refusal = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "nx terminal body status index",
        |ctx| terminal_feature_body_ids(ctx, &BTreeSet::new(), &bindings[..1], &[]),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = refusal else {
        panic!("the first status lookup must have a work refusal boundary");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "nx terminal body status index");
    let prefix_work = limit.used + limit.additional;

    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = prefix_work,
        |ctx| {
            assert!(terminal_feature_body_ids(ctx, &BTreeSet::new(), &bindings, &[])
                .unwrap()
                .is_none());
            assert_eq!(ctx.resource_refusal(), None);
        },
    );
}

#[test]
fn terminal_body_selection_late_missing_binding_retains_no_discarded_identity() {
    let (emitted, bindings, statuses) = one_terminal_body();
    let mut missing = bindings[0].clone();
    missing.id = "binding#missing".into();
    missing.stream_ordinal = 1;
    let bindings = [bindings[0].clone(), missing];
    crate::test_support::with_decode_context_over(&[], |policy| {
        policy.limits.max_retained_bytes = 0;
    }, |ctx| {
        assert!(terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn terminal_body_selection_unmatched_status_retains_no_discarded_identity() {
    let (emitted, bindings, statuses) = one_terminal_body();
    let mut unmatched = statuses[0].clone();
    unmatched.id = "status#extra".into();
    unmatched.segment_body_binding = "binding#extra".into();
    let statuses = [statuses[0].clone(), unmatched];
    crate::test_support::with_decode_context_over(&[], |policy| {
        policy.limits.max_retained_bytes = 0;
    }, |ctx| {
        assert!(terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn terminal_body_selection_unmapped_body_retains_no_discarded_identity() {
    let (mut emitted, bindings, statuses) = one_terminal_body();
    emitted.insert(BodyId::mint("nx:s1:body#0").unwrap());
    crate::test_support::with_decode_context_over(&[], |policy| {
        policy.limits.max_retained_bytes = 0;
    }, |ctx| {
        assert!(terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn terminal_body_selection_combines_terminal_flags_for_one_stream() {
    let (emitted, bindings, statuses) = one_terminal_body();
    let mut second = bindings[0].clone();
    second.id = "binding#1".into();
    let bindings = [bindings[0].clone(), second];
    let mut second_status = statuses[0].clone();
    second_status.id = "status#1".into();
    second_status.segment_body_binding = bindings[1].id.clone();
    for first_terminal in [false, true] {
        let mut statuses = [statuses[0].clone(), second_status.clone()];
        statuses[0].terminal = first_terminal;
        statuses[1].terminal = !first_terminal;
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses).unwrap(), Some(emitted.clone()));
            assert!(ctx.resource_refusal().is_none());
        });
    }
}

#[test]
fn terminal_body_selection_without_terminal_bodies_skips_output_traversal() {
    let (emitted, bindings, mut statuses) = one_terminal_body();
    statuses[0].terminal = false;
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "nx selected terminal body traversal",
        None,
    );
    crate::test_support::with_decode_context_over(&[], |policy| {
        policy.limits.max_work_units = u64::MAX;
        policy.limits.max_retained_bytes = 0;
    }, |ctx| {
        assert!(terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn terminal_body_selection_preserves_first_match_identity_copy_order() {
    let (mut emitted, bindings, statuses) = one_terminal_body();
    let longer_body = BodyId::mint("nx:s1:body#10000").unwrap();
    emitted.insert(longer_body.clone());
    let mut second_binding = bindings[0].clone();
    second_binding.id = "binding#1".into();
    second_binding.stream_ordinal = 1;
    let mut second_status = statuses[0].clone();
    second_status.id = "status#1".into();
    second_status.segment_body_binding = second_binding.id.clone();
    let bindings = [second_binding, bindings[0].clone()];
    let statuses = [statuses[0].clone(), second_status];
    let first_bytes = cadmpeg_core::decode::u64_from_index(longer_body.as_str().len());
    crate::test_support::with_decode_context_over(&[], |policy| {
        policy.limits.max_retained_bytes = first_bytes - 1;
    }, |ctx| {
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses)
        else {
            panic!("the first binding selects the longer body identity");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "nx selected terminal body identity");
        assert_eq!((limit.used, limit.additional), (0, first_bytes));
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses).unwrap(), Some(emitted));
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn terminal_body_selection_refuses_scan_link_and_projection_work() {
    let (mut emitted, bindings, statuses) = one_terminal_body();
    emitted.insert(BodyId::mint("nx:s0:body#1").unwrap());
    for operation in ["nx terminal body scan", "nx terminal body selection link", "nx selected terminal body traversal"] {
        let error = crate::test_support::resource_refusal_at(
            &[], ResourceDimension::WorkUnits, operation,
            |ctx| terminal_feature_body_ids(ctx, &emitted, &bindings, &statuses),
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("ordering and output lookups must propagate resource refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, operation);
        if operation == "nx terminal body scan" {
            assert_eq!(limit.additional, 1);
        }
    }
}
