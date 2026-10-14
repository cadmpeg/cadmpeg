// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;

use super::{admit_evaluation_cycles, CarrierConstructions};
use crate::report::check::Check;
use crate::test_support::evaluation_cycles::cyclic_model;
use crate::validate::{admit, validate_neutral};

#[test]
fn model_admission_refuses_a_malformed_curve_surface_reference_cycle() {
    let (ir, curve, surface) = cyclic_model();
    let expected =
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}");
    let report = validate_neutral(&ir, Vec::new()).expect("resource allocation did not fail");
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity && finding.message == expected
    }));
    assert!(!admit::admit(
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        admit::DRAFT_CORE_CHECKS,
        Vec::new()
    )
    .expect("resource allocation did not fail")
    .is_ok());
    assert!(matches!(
        admit_evaluation_cycles(&cadmpeg_test_support::service_decode_context(), &ir),
        Err(CodecError::Malformed(message)) if message == expected
    ));
}

#[test]
fn cycle_admission_preserves_zero_depth_session_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (ir, _, _) = cyclic_model();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(CodecError::ResourceLimit(limit)) = admit_evaluation_cycles(&ctx, &ir) else {
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
    ir.model.curves.push(ir.model.curves[0].clone());
    let index = crate::index::ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check_evaluation_cycles(&ctx, &ir, &index, &mut findings).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].check, Check::ReferentialIntegrity);
    assert_eq!(
        findings[0].message,
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}")
    );
}

#[test]
fn cycle_admission_admits_solved_carriers_among_many_points() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut ir = crate::examples::unit_cube().expect("cube fixture");
    let point = ir.model.points[0].clone();
    ir.model.points.extend(std::iter::repeat_n(point, 1_024));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    admit_evaluation_cycles(&ctx, &ir).expect("solved carriers have no cycle index slots");
    ctx.finish_session().expect("unfused session");
}

#[test]
fn cycle_admission_finds_cycles_among_many_unrelated_points() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (mut ir, curve, surface) = cyclic_model();
    let cube = crate::examples::unit_cube().expect("cube fixture");
    ir.model
        .points
        .extend(std::iter::repeat_n(cube.model.points[0].clone(), 1_024));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    assert!(
        matches!(admit_evaluation_cycles(&ctx, &ir), Err(CodecError::Malformed(message))
        if message == format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}"))
    );
    ctx.finish_session()
        .expect("semantic refusal does not fuse resources");
}

#[test]
fn cycle_associations_keep_the_general_index_duplicate_selection_rules() {
    use crate::geometry::{
        CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
        ProceduralSurfaceDefinition, SurfaceGeometry,
    };
    use crate::ids::{ProceduralCurveId, ProceduralSurfaceId};
    let (mut ir, curve, surface) = cyclic_model();
    // Duplicate construction IDs select the first construction row.
    ir.model.procedural_curves.push(ProceduralCurve::new(
        ir.model.procedural_curves[0].id.clone(),
        ProceduralCurveDefinition::Exact { cache: None },
    ));
    // Duplicate curve carriers select the first resolved construction.
    let other_curve = ProceduralCurveId::mint("test:cycle:construction#other-curve").unwrap();
    ir.model.procedural_curves.push(ProceduralCurve::new(
        other_curve.clone(),
        ProceduralCurveDefinition::Exact { cache: None },
    ));
    let mut carrier = ir.model.curves[0].clone();
    carrier.geometry = CurveGeometry::Procedural {
        construction: other_curve,
        cache: None,
    };
    ir.model.curves.push(carrier);
    // Duplicate surface carriers select the last resolved construction.
    let other_surface = ProceduralSurfaceId::mint("test:cycle:construction#other-surface").unwrap();
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        other_surface.clone(),
        ProceduralSurfaceDefinition::Replica {
            source: surface.clone(),
            transform: crate::transform::Transform::identity(),
        },
        None,
    ));
    let mut carrier = ir.model.surfaces[0].clone();
    carrier.geometry = SurfaceGeometry::Procedural {
        construction: other_surface,
        cache: None,
    };
    ir.model.surfaces.push(carrier);
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = super::CycleIndex::build(&ctx, &ir).unwrap();
    let general = crate::index::ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    assert_eq!(
        index.curve(&ctx, curve.as_str()).unwrap(),
        general.curve(&ctx, curve.as_str()).unwrap()
    );
    assert_eq!(
        index.surface(&ctx, surface.as_str()).unwrap(),
        general.surface(&ctx, surface.as_str()).unwrap()
    );
    assert_eq!(
        index.curve(&ctx, curve.as_str()).unwrap(),
        Some(&ir.model.procedural_curves[0])
    );
    assert_eq!(
        index.surface(&ctx, surface.as_str()).unwrap(),
        Some(&ir.model.procedural_surfaces[1])
    );
}
