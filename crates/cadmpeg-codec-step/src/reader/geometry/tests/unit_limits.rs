// SPDX-License-Identifier: Apache-2.0
//! Caller-budget refusals for STEP unit resolution.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";
const LENGTH: &str = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));";
const ANGLE: &str = "#1=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));";

fn unit_refusal(records: &str, angle: bool, collection_limit: u64, depth_limit: Option<u64>) -> CodecError {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("valid unit exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    if let Some(limit) = depth_limit {
        policy.limits.max_recursion_depth = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut active = BTreeSet::new();
    let result = if angle {
        super::super::unit_scale_radians(1, &exchange, &mut active, Some(&ctx))
    } else {
        super::super::unit_scale_mm(1, &exchange, &mut active, Some(&ctx))
    };
    result.expect_err("unit resolution exceeds the limit")
}

#[test]
fn length_unit_active_refuses_collection_limit() {
    assert!(matches!(
        unit_refusal(LENGTH, false, 0, None),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_length_unit_active"
    ));
}

#[test]
fn angle_unit_active_refuses_collection_limit() {
    assert!(matches!(
        unit_refusal(ANGLE, true, 0, None),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_angle_unit_active"
    ));
}

#[test]
fn length_unit_scale_walk_refuses_depth_limit() {
    assert!(matches!(
        unit_refusal(LENGTH, false, 1, Some(0)),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_length_unit_scale_walk"
    ));
}

#[test]
fn angle_unit_scale_walk_refuses_depth_limit() {
    assert!(matches!(
        unit_refusal(ANGLE, true, 1, Some(0)),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_angle_unit_scale_walk"
    ));
}

fn document_refusal(records: &str, collection_limit: u64, with_context: bool) -> CodecError {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("valid document units");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let result = super::super::document_unit_scale(
        &exchange,
        "LENGTH_UNIT",
        super::super::unit_scale_mm,
        &ctx,
    );
    assert_eq!(with_context, records.contains("GLOBAL_UNIT_ASSIGNED_CONTEXT"));
    result.expect_err("document unit collection exceeds the limit")
}

#[test]
fn document_unit_ids_refuse_collection_limit() {
    let records = format!("{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));");
    assert!(matches!(
        document_refusal(&records, 0, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_document_unit_ids"
    ));
}

#[test]
fn document_unit_scales_refuse_collection_limit() {
    let records = format!("{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));");
    assert!(matches!(
        document_refusal(&records, 2, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_document_unit_scales"
    ));
}

#[test]
fn document_context_scales_refuse_collection_limit() {
    let records = format!("{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));");
    assert!(matches!(
        document_refusal(&records, 3, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_document_context_scales"
    ));
}

#[test]
fn document_fallback_scales_refuse_collection_limit() {
    assert!(matches!(
        document_refusal(LENGTH, 1, false),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_document_fallback_scales"
    ));
}

#[test]
fn context_length_scales_refuse_collection_limit() {
    let records = format!("{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));");
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("valid context units");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::context_unit_scales(2, &exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_context_length_scales"
    ));
}

#[test]
fn context_angle_scales_refuse_collection_limit() {
    let records = format!("{ANGLE}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));");
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) = crate::parse::parse(source.as_bytes()).expect("valid context units");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::context_unit_scales(2, &exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_context_angle_scales"
    ));
}
