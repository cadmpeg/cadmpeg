// SPDX-License-Identifier: Apache-2.0
//! Recursion-depth refusals in the STEP geometry reader.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn pcurve_geometry_refusal(depth_limit: u64) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_CURVE();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid curve record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::decode_pcurve_geometry(
        1,
        &exchange,
        super::super::PcurveSources {
            points: &BTreeMap::new(),
            vectors: &BTreeMap::new(),
            placements: &BTreeMap::new(),
            transformations: &BTreeMap::new(),
            angle_scale: 1.0,
        },
        &mut Vec::new(),
        &mut BTreeSet::new(),
        &ctx,
    )
    .expect_err("pcurve geometry exceeds the limit")
}

#[test]
fn pcurve_geometry_walk_refuses_depth_limit() {
    assert!(
        matches!(pcurve_geometry_refusal(0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_pcurve_geometry_walk")
    );
}

fn surface_scale_refusal(depth_limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let ir = cadmpeg_ir::document::CadIr::empty();
    let id = cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#1").expect("valid identity");
    super::super::surface_parameter_scales_for_step(
        &ir,
        &id,
        &cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
        ),
        1.0,
        1.0,
        &BTreeMap::new(),
        &ctx,
    )
    .expect_err("surface scale exceeds limit")
}

#[test]
fn surface_parameter_scale_walk_refuses_depth_limit() {
    assert!(
        matches!(surface_scale_refusal(0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_surface_parameter_scale_walk")
    );
}

#[test]
fn surface_geometry_scale_walk_refuses_depth_limit() {
    assert!(
        matches!(surface_scale_refusal(1), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_surface_geometry_scale_walk")
    );
}

fn directrix_scale_refusal(depth_limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let ir = cadmpeg_ir::document::CadIr::empty();
    let id = cadmpeg_ir::ids::CurveId::mint("test:model:curve#1").expect("valid identity");
    super::super::directrix_parameter_scale_inner(&ir, &id, 1.0, 1.0, &mut BTreeSet::new(), &ctx)
        .expect_err("directrix scale exceeds limit")
}

#[test]
fn directrix_scale_walk_refuses_depth_limit() {
    assert!(
        matches!(directrix_scale_refusal(0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_directrix_scale_walk")
    );
}

#[test]
fn directrix_geometry_scale_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::directrix_geometry_parameter_scale(
            &cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
            1.0, 1.0, &ctx,
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_directrix_geometry_scale_walk"
    ));
}

fn line_scale_refusal(source: &[u8], depth_limit: u64) -> CodecError {
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid curve record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::line_parameter_scale(
        &exchange,
        1,
        cadmpeg_ir::scalar::PositiveReal::new(1.0).expect("positive scale"),
        &mut Vec::new(),
        &ctx,
    )
    .expect_err("line scale exceeds limit")
}

#[test]
fn line_parameter_scale_walk_refuses_depth_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_CURVE();ENDSEC;END-ISO-10303-21;";
    assert!(
        matches!(line_scale_refusal(source, 0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_line_parameter_scale_walk")
    );
}

#[test]
fn trim_parameter_scale_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(super::super::parameter_scale(
        &cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None }, 1.0, 1.0, &ctx,
    ), Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_trim_parameter_scale_walk"));
}

#[test]
fn trim_parameter_value_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let points = BTreeMap::new();
    let geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
    );
    let mut losses = Vec::new();
    let context = super::super::TrimParameterContext {
        points: &points,
        geometry: &geometry,
        angle_scale: 1.0,
        linear_parameter_scale: 1.0,
        parameter_offset: 0.0,
        tolerance: 1.0,
        master_representation: super::super::TrimMasterRepresentation::Parameter,
        record_id: 1,
        losses: &mut losses,
        ctx: &ctx,
    };
    assert!(matches!(
        super::super::trim_parameter_value(&crate::parse::Value::Integer(1), &context),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_trim_parameter_value_walk"
    ));
}
