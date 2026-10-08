// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::test_support::test_surface_fixtures::bounded_plane_file;

#[test]
fn trimming_projection_refuses_retained_boundary_source_text() {
    let bytes = bounded_plane_file();
    let derivation = crate::test_support::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).unwrap();
        let (global, _, _global_storage) = crate::global::parse(&scan, ctx).unwrap();
        let (directory, quarantined) =
            crate::directory::parse(&scan, global.global_table(), ctx).unwrap();
        assert!(quarantined.is_empty());
        let parameters = crate::parameter::assemble_with_context(
            &scan, &directory, &quarantined, &global, ctx,
        ).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let projection = super::super::project_geometry(
            &mut ir, &directory, &parameters.records,
            &parameters.trailing_pointer_analysis,
            &global.length_context().unwrap(), ctx,
        ).unwrap();
        projection.boundary_vertex_derivations[0].clone()
    });
    let members: Vec<_> = (0..derivation.source_endpoints.len()).collect();
    for (operation, expected) in [
        ("iges boundary derivation edge text", derivation.source_endpoints[0].edge.len()),
        ("iges boundary derivation source text", derivation.source_entity.len()),
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes, operation, |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                policy.limits.max_retained_bytes = 0;
                crate::test_support::with_policy_context(&[], &policy, |ctx| {
                    let mut storage = ctx.reserve_scoped(0, "test boundary derivation storage")?;
                    super::super::BoundaryVertexDerivation::for_decode(
                        (&derivation.source_entity, &derivation.vertex),
                        derivation.representative, derivation.tolerance,
                        &derivation.source_endpoints, &members, ctx, &mut storage,
                    ).map(|_| ())
                })
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.additional == u64::try_from(expected).unwrap()));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test boundary derivation storage").unwrap();
    let copy = super::super::BoundaryVertexDerivation::for_decode(
        (&derivation.source_entity, &derivation.vertex),
        derivation.representative, derivation.tolerance,
        &derivation.source_endpoints, &members, &ctx, &mut storage,
    ).unwrap();
    assert_eq!(copy.source_entity, derivation.source_entity);
    assert_eq!(copy.vertex, derivation.vertex);
    assert_eq!(copy.source_endpoints.len(), derivation.source_endpoints.len());
    drop(copy);
    drop(storage);
    ctx.finish_session().unwrap();
}
