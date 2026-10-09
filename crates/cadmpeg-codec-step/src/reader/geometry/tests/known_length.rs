// SPDX-License-Identifier: Apache-2.0
//! Geometry work admission follows the executed fixed-step prefix.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

use super::super::{default_nurbs_knots, expand_knots, topology_owned_carriers, DefaultNurbsKnotKind};
use crate::parse::Value;

fn assert_first_knot_refusal(kind: Option<DefaultNurbsKnotKind>) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One distinct-knot visit and one repetition execute before slot refusal.
    policy.limits.max_work_units = 2;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let result = if let Some(kind) = kind {
        default_nurbs_knots(2, 1, kind, &ctx)
    } else {
        let counts = Value::List(vec![Value::Integer(2)]);
        let distinct = Value::List(vec![Value::Real(FiniteReal::new(0.0).expect("finite knot"))]);
        expand_knots(&counts, &distinct, 2, &ctx)
    };
    let CodecError::ResourceLimit(first) = result.expect_err("first knot slot refuses")
        else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, if kind.is_some() {
        "step_default_nurbs_knots"
    } else {
        "step_expanded_nurbs_knots"
    });
    assert_eq!(first.used, 0);
    assert_eq!(first.additional, 1);
    assert_eq!(first.limit, 0);
    assert_eq!(ctx.resource_refusal(), Some(first));
    assert!(matches!(ctx.charge_work(0, "test original refusal"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn quasi_uniform_knots_refuse_first_slot_without_admitting_unused_repetitions() {
    assert_first_knot_refusal(Some(DefaultNurbsKnotKind::QuasiUniform));
}

#[test]
fn bezier_knots_refuse_first_slot_without_admitting_unused_repetitions() {
    assert_first_knot_refusal(Some(DefaultNurbsKnotKind::Bezier));
}

#[test]
fn explicit_knots_refuse_first_slot_without_admitting_unused_repetitions() {
    assert_first_knot_refusal(None);
}

#[test]
fn empty_owned_carriers_require_no_work_or_terminal_probe() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let ir = cadmpeg_ir::CadIr::empty();
    let index = crate::reader::index::CarrierIndex {
        curves: std::collections::HashMap::new(),
        points: std::collections::HashMap::new(),
        surfaces: std::collections::HashMap::new(),
    };
    let owned = topology_owned_carriers(&ir, &index, &ctx).expect("empty bases require no work");
    assert!(owned.curves.is_empty());
    assert!(owned.surfaces.is_empty());
    assert!(owned.points.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    drop(owned);
    ctx.finish_session().expect("empty scratch released");
}

#[test]
fn empty_owned_carriers_preserve_original_sticky_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let CodecError::ResourceLimit(first) = ctx.charge_work(1, "test original work refusal")
        .expect_err("original refusal") else { panic!("resource refusal") };
    let ir = cadmpeg_ir::CadIr::empty();
    let index = crate::reader::index::CarrierIndex {
        curves: std::collections::HashMap::new(),
        points: std::collections::HashMap::new(),
        surfaces: std::collections::HashMap::new(),
    };
    let error = match topology_owned_carriers(&ir, &index, &ctx) {
        Ok(_) => panic!("refused context"),
        Err(error) => error,
    };
    assert!(matches!(error, CodecError::ResourceLimit(sticky) if sticky == first));
    assert_eq!(ctx.resource_refusal(), Some(first));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}
