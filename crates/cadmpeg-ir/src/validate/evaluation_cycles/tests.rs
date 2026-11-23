// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;

use super::check_evaluation_cycles;
use crate::index::ModelIndex;
use crate::report::check::Check;
use crate::test_support::evaluation_cycles::cyclic_model;
use crate::validate::{admit, validate_neutral};
use crate::CadIr;

fn cycle_findings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Vec<crate::report::check::Finding>, CodecError> {
    let index = ModelIndex::new_model_only(ir);
    let mut findings = Vec::new();
    check_evaluation_cycles(ctx, ir, &index, &mut findings)?;
    Ok(findings)
}

#[test]
fn validation_reports_a_malformed_curve_surface_reference_cycle() {
    let (ir, curve, surface) = cyclic_model();
    let expected =
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}");
    let report = validate_neutral(&ir, Vec::new()).expect("validate the neutral model");
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity && finding.message == expected
    }));
    assert!(!admit::admit(
        &cadmpeg_core::decode::test_support::service_decode_context(),
        &ir,
        admit::DRAFT_CORE_CHECKS,
        Vec::new()
    )
    .expect("admit the model")
    .is_ok());
}

#[test]
fn cycle_check_preserves_zero_depth_session_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (ir, _, _) = cyclic_model();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(CodecError::ResourceLimit(limit)) = cycle_findings(&ctx, &ir) else {
        panic!("traversal frame must be refused");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.operation, "cycle traversal depth");
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}

#[test]
fn cycle_check_reports_one_cycle_for_repeated_carrier_rows() {
    let (mut ir, curve, surface) = cyclic_model();
    // Repeated carrier rows still share the last graph entry and one active state.
    ir.geometry.curves.push(ir.geometry.curves[0].clone());
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let findings = cycle_findings(&ctx, &ir).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].check, Check::ReferentialIntegrity);
    assert_eq!(findings[0].entity.as_deref(), Some(curve.as_str()));
    assert_eq!(findings[0].related, vec![surface.as_str()]);
    assert_eq!(
        findings[0].message,
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}")
    );
}

#[test]
fn cycle_check_finds_no_cycle_among_solved_carriers() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut ir = crate::examples::unit_cube().expect("cube fixture");
    let point = ir.geometry.points[0].clone();
    ir.geometry.points.extend(std::iter::repeat_n(point, 1_024));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    assert!(cycle_findings(&ctx, &ir)
        .expect("solved carriers walk")
        .is_empty());
    ctx.finish_session().expect("unfused session");
}

#[test]
fn cycle_check_finds_cycles_among_many_unrelated_points() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (mut ir, curve, surface) = cyclic_model();
    let cube = crate::examples::unit_cube().expect("cube fixture");
    ir.geometry
        .points
        .extend(std::iter::repeat_n(cube.geometry.points[0].clone(), 1_024));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    let findings = cycle_findings(&ctx, &ir).expect("cyclic model walk");
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>(),
        [
            format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}")
                .as_str()
        ]
    );
    ctx.finish_session()
        .expect("a cycle finding does not fuse resources");
}
