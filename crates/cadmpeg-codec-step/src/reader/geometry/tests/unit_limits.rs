// SPDX-License-Identifier: Apache-2.0
//! Recursion-depth refusals and shared-scope reuse in STEP unit resolution.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";
const LENGTH: &str = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));";
const ANGLE: &str = "#1=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));";

fn unit_refusal(records: &str, angle: bool, depth_limit: u64) -> CodecError {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid unit exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut active = BTreeSet::new();
    let result = if angle {
        super::super::unit_scale_radians(1, &exchange, &mut active, &ctx)
    } else {
        super::super::unit_scale_mm(1, &exchange, &mut active, &ctx)
    };
    result.expect_err("unit resolution exceeds the limit")
}

#[test]
fn length_unit_scale_walk_refuses_depth_limit() {
    assert!(matches!(
        unit_refusal(LENGTH, false, 0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_length_unit_scale_walk"
    ));
}

#[test]
fn angle_unit_scale_walk_refuses_depth_limit() {
    assert!(matches!(
        unit_refusal(ANGLE, true, 0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_angle_unit_scale_walk"
    ));
}

fn scope_refusal(records: &str, depth_limit: u64) -> CodecError {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid scope exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    super::super::collect_unit_scope_members(1, &exchange, &mut BTreeSet::new(), &ctx)
        .expect_err("scope traversal exceeds the limit")
}

#[test]
fn unit_scope_reference_walk_refuses_depth_limit() {
    assert!(matches!(scope_refusal("#1=ITEM(#2);#2=ITEM();", 1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_reference_value_walk"));
}

#[test]
fn shared_geometry_is_visited_once_per_unit_scope() {
    use std::fmt::Write;
    let mut records = format!("{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));#3=CARTESIAN_POINT('',(1.,2.,3.));");
    for id in 10..1010 {
        write!(records, "#{id}=GEOMETRIC_SET((#3));").expect("write fixture");
    }
    records.push_str("#4=SHAPE_REPRESENTATION('',(");
    for id in 10..1010 {
        if id != 10 {
            records.push(',');
        }
        write!(records, "#{id}").expect("write fixture");
    }
    records.push_str("),#2);");
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("exchange");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).expect("context");
    let scales = super::super::resolve_unit_scales(
        &exchange,
        PositiveReal::ONE,
        PositiveReal::ONE,
        &mut Vec::new(),
        &ctx,
    )
    .expect("shared members fit linear budget");
    assert_eq!(scales.length([3]), PositiveReal::ONE);
}

#[test]
fn repeated_unit_candidates_keep_one_first_scale_and_a_final_conflict() {
    let mut candidates = BTreeMap::new();
    for _ in 0..1_000 {
        super::super::add_unit_candidate(&mut candidates, 1, PositiveReal::ONE);
    }
    assert_eq!(candidates[&1], Some(PositiveReal::ONE));
    super::super::add_unit_candidate(
        &mut candidates,
        1,
        PositiveReal::new(2.0).expect("positive"),
    );
    super::super::add_unit_candidate(&mut candidates, 1, PositiveReal::ONE);
    assert_eq!(candidates[&1], None);
}

#[test]
fn unit_scope_reuses_members_for_shared_children_and_cycles() {
    let source = format!("{HEADER}#1=ITEM((#2,#2));#2=ITEM((#1,#3));#3=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("cyclic authored scope");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut members = BTreeSet::new();
    super::super::collect_unit_scope_members(1, &exchange, &mut members, &ctx)
        .expect("each scope member is stored once");
    assert_eq!(members, BTreeSet::from([1, 2, 3]));
}

#[test]
fn uniform_default_units_skip_geometry_scope_bookkeeping() {
    use std::fmt::Write;

    let mut source = format!("{HEADER}{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('',''));#3=CARTESIAN_POINT('',(1.,2.,3.));");
    for id in 100..300 {
        write!(source, "#{id}=SHAPE_REPRESENTATION('',(#3),#2);").expect("authored representation");
    }
    source.push_str(TAIL);
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("shared authored context");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut losses = Vec::new();
    let scales = super::super::resolve_unit_scales(
        &exchange,
        PositiveReal::ONE,
        PositiveReal::ONE,
        &mut losses,
        &ctx,
    )
    .expect("one context admission; no per-representation geometry collections");
    assert!(scales.length.is_empty());
    assert!(scales.angle.is_empty());
    assert!(losses.is_empty());
}
