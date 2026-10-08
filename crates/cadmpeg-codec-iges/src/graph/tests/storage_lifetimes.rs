// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{
    directory_target, ExpectationLabel, ParameterResolver, ReferenceEdge, ReferenceExpectation,
    ReferenceKind, ReferenceOrigin, Resolution,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use std::collections::BTreeMap;

#[test]
fn parameter_append_releases_consumed_source_vector_backing() {
    const SOURCE_COUNT: u32 = 12;
    const MATERIALIZED_LIMIT: u64 = 8192;
    let directory = [directory_target(1, 116)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = MATERIALIZED_LIMIT;
    for refuse_while_live in [true, false] {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
    let mut graph = BTreeMap::new();
    for source in 1..=SOURCE_COUNT {
        graph.insert(source, vec![ReferenceEdge {
            origin: ReferenceOrigin::Directory(ReferenceKind::Transform),
            raw_pointer: 1,
            resolution: Resolution::Resolved(1),
            expected: ReferenceExpectation::Named(ExpectationLabel::ExistingDirectoryEntry),
        }]);
        assert_eq!(resolver.resolve_any(source, 0, 1).unwrap(), Some(1));
    }
    let storage = resolver.append_to(&mut graph).unwrap();
    for edges in graph.values() {
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[1].origin, ReferenceOrigin::Parameter { index: 0 });
        assert_eq!(edges[1].resolution, Resolution::Resolved(1));
    }
    // Each target starts with one slot. Admitted amortized growth adds three
    // slots. Source buffers and their source-group tree no longer exist.
    let live_growth = u64::from(SOURCE_COUNT) * 3
        * u64::try_from(std::mem::size_of::<ReferenceEdge>()).unwrap();
    let remaining = ctx.reserve_scoped(
        MATERIALIZED_LIMIT - live_growth,
        "remaining materialized storage after parameter append",
    ).unwrap();
    if refuse_while_live {
        let error = ctx.reserve_scoped(1, "live appended parameter growth").unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(expected) = error else {
            panic!("expected materialized refusal");
        };
        assert_eq!(expected.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(expected.operation, "live appended parameter growth");
        assert_eq!(expected.used, MATERIALIZED_LIMIT);
        assert_eq!(expected.additional, 1);
        drop(remaining);
        drop(graph);
        drop(storage);
        assert!(matches!(ctx.finish_session().unwrap_err(),
            cadmpeg_core::CodecError::ResourceLimit(actual) if actual == expected
        ));
    } else {
        drop(remaining);
        drop(graph);
        drop(storage);
        ctx.reserve_scoped(MATERIALIZED_LIMIT, "released appended parameter graph").unwrap();
        ctx.finish_session().unwrap();
    }
    }
}

#[test]
fn parameter_append_keeps_absent_bucket_storage_live() {
    const MATERIALIZED_LIMIT: u64 = 8192;
    let directory = [directory_target(1, 116)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = MATERIALIZED_LIMIT;
    for refuse_while_live in [true, false] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let resolver = ParameterResolver::new(&directory, &ctx).unwrap();
        assert_eq!(resolver.resolve_any(1, 0, 1).unwrap(), Some(1));
        let mut graph = BTreeMap::new();
        let storage = resolver.append_to(&mut graph).unwrap();
        assert_eq!(graph[&1].len(), 1);
        assert_eq!(graph[&1][0].resolution, Resolution::Resolved(1));
        if refuse_while_live {
            let error = ctx.reserve_scoped(
                MATERIALIZED_LIMIT,
                "live parameter graph bucket",
            ).unwrap_err();
            assert!(matches!(error,
                cadmpeg_core::CodecError::ResourceLimit(ref limit)
                    if limit.dimension == ResourceDimension::MaterializedBytes
                        && limit.operation == "live parameter graph bucket"
            ));
            let cadmpeg_core::CodecError::ResourceLimit(expected) = error else {
                panic!("expected materialized refusal");
            };
            drop(graph);
            drop(storage);
            assert!(matches!(ctx.finish_session().unwrap_err(),
                cadmpeg_core::CodecError::ResourceLimit(actual) if actual == expected
            ));
        } else {
            drop(graph);
            drop(storage);
            ctx.reserve_scoped(MATERIALIZED_LIMIT, "released parameter graph bucket").unwrap();
            ctx.finish_session().unwrap();
        }
    }
}
