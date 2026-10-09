// SPDX-License-Identifier: Apache-2.0

use super::super::{single_pcurve_use, PcurveSink};
use crate::brep::graph::Brep;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::DirectedParameterRange;
use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId, PcurveId};
use cadmpeg_ir::topology::{Coedge, PcurveUse, Sense};

const OPERATION: &str = "collect intersection Parasolid pcurve uses";

fn one_use() -> PcurveUse {
    PcurveUse {
        pcurve: PcurveId::mint("test:model:pcurve#binding").unwrap(),
        isoparametric: None,
        parameter_range: Some(DirectedParameterRange::new([1., 0.]).unwrap()),
    }
}

fn use_bytes() -> u64 {
    // collection_vec uses exact one-element backing without amortized growth.
    u64::try_from(std::mem::size_of::<PcurveUse>()).unwrap()
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 1;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = use_bytes();
    policy
}

fn one_coedge() -> Brep {
    let id = CoedgeId::mint("test:model:coedge#binding").unwrap();
    Brep {
        coedges: vec![Coedge {
            id: id.clone(),
            owner_loop: LoopId::mint("test:model:loop#binding").unwrap(),
            edge: EdgeId::mint("test:model:edge#binding").unwrap(),
            radial_next: id,
            sense: Sense::Forward,
            use_curve: None,
            pcurves: Vec::new(),
        }],
        ..Brep::default()
    }
}

#[test]
fn singleton_pcurve_use_has_exact_retained_backing_and_free_fixed_work() {
    let value = one_use();
    let expected = value.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let uses = single_pcurve_use(&ctx, value, OPERATION).unwrap();
    assert_eq!(uses, [expected]);
    assert_eq!(uses.capacity(), 1);
    ctx.finish_session().unwrap();
}

#[test]
fn singleton_pcurve_use_refuses_retention_before_allocation() {
    let value = one_use();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes -= 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| single_pcurve_use(&ctx, value, OPERATION));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("exact singleton backing"); };
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional, limit.limit), (0, use_bytes(), use_bytes() - 1));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn singleton_pcurve_use_refuses_its_collection_slot_before_allocation() {
    let value = one_use();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| single_pcurve_use(&ctx, value, OPERATION));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("singleton slot"); };
    assert_eq!(limit.operation, OPERATION);
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!((limit.used, limit.additional, limit.limit), (0, 1, 0));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn singleton_pcurve_use_preserves_prior_refusal_before_allocation() {
    let value = one_use();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(2, "test original singleton refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| single_pcurve_use(&ctx, value, OPERATION));
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn derived_pcurve_binding_uses_the_same_exact_singleton_owner() {
    let mut out = one_coedge();
    let value = one_use();
    let expected = value.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    PcurveSink { coedges: &mut out.coedges, pcurves: &mut out.pcurves, losses: &mut out.losses, stats: &mut out.stats }
        .bind(&ctx, 0, value.pcurve, value.parameter_range).unwrap();
    assert_eq!(out.coedges[0].pcurves, [expected]);
    assert_eq!(out.coedges[0].pcurves.capacity(), 1);
    ctx.finish_session().unwrap();
}

#[test]
fn refused_derived_pcurve_binding_does_not_mutate_the_coedge() {
    let mut out = one_coedge();
    let original = out.coedges[0].clone();
    let value = one_use();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes -= 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        PcurveSink { coedges: &mut out.coedges, pcurves: &mut out.pcurves, losses: &mut out.losses, stats: &mut out.stats }
            .bind(&ctx, 0, value.pcurve, value.parameter_range)
    });
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("derived singleton backing"); };
    assert_eq!(limit.operation, "bind derived Parasolid pcurve");
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((limit.used, limit.additional, limit.limit), (0, use_bytes(), use_bytes() - 1));
    assert_eq!(out.coedges[0], original);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
