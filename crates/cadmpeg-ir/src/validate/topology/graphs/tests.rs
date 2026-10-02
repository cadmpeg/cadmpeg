// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::document::CadIr;
use crate::report::{check::{Check, Finding}, Severity};

fn refuses(ir: &CadIr, check: impl Fn(&DecodeContext<'_>, &CadIr, &mut Vec<Finding>) -> Result<(), CodecError>) {
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits, ResourceDimension::RetainedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = check(&ctx, ir, &mut findings) else { panic!("topology graph must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn radial_ring_validation_preserves_index_walk_and_finding_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.coedges[0].radial_next = "test:model:coedge#missing".try_into().unwrap();
    refuses(&ir, super::check_coedge_pairing);
}

#[test]
fn wire_topology_validation_preserves_index_walk_and_finding_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.shells[0].add_wire_edge(ir.model.coedges[0].edge.clone());
    refuses(&ir, super::check_wire_topology);
}

#[test]
fn shell_connectivity_validation_preserves_index_walk_and_finding_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.coedges.clear();
    refuses(&ir, super::check_shell_connectivity);
}

#[test]
fn topology_graph_validation_releases_scopes_without_retaining_identities() {
    let ir = crate::examples::unit_cube().unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 32768;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::check_coedge_pairing(&ctx, &ir, &mut findings).unwrap();
    super::check_wire_topology(&ctx, &ir, &mut findings).unwrap();
    super::check_shell_connectivity(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(ctx.reserve_scoped(32768, "topology graph scopes released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn radial_ring_crossing_keeps_error_order_and_owner() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let owner = ir.model.coedges[0].id.clone();
    let other = ir.model.coedges.iter().find(|coedge| coedge.edge != ir.model.coedges[0].edge).unwrap().id.clone();
    ir.model.coedges[0].radial_next = other;
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check_coedge_pairing(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.len() >= 2);
    for finding in &findings[..2] {
        assert_eq!(finding.check, Check::CoedgePairing);
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.entity.as_deref(), Some(owner.as_str()));
    }
    assert_eq!(findings[0].message, "radial ring crosses edges");
    assert_eq!(findings[1].message, "radial ring does not close");
}
