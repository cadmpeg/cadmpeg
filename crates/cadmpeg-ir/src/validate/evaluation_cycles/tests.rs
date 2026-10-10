// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;

use super::admit_evaluation_cycles;
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
fn cycle_admission_preserves_zero_collection_session_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (ir, _, _) = cyclic_model();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(CodecError::ResourceLimit(limit)) = admit_evaluation_cycles(&ctx, &ir) else {
        panic!("index must be refused");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
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
fn cycle_walk_preserves_each_resource_refusal_before_emitting_a_finding() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (ir, _, _) = cyclic_model();
    let index = crate::index::ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("test dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let emitted = std::cell::Cell::new(false);
        let result = super::walk_cycles(&ctx, &ir, &index, |_| {
            emitted.set(true);
            Ok(false)
        });
        let Err(CodecError::ResourceLimit(original)) = result else {
            panic!("cycle walk must preserve the original resource error");
        };
        assert_eq!(original.dimension, dimension);
        assert!(!emitted.get());
        assert!(
            matches!(super::walk_cycles(&ctx, &ir, &index, |_| Ok(false)), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
        );
    }
}

#[test]
fn cycle_walk_keeps_byte_order_stops_at_first_cycle_and_releases_graph_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (mut ir, curve, surface) = cyclic_model();
    // Repeated carrier rows still share the last graph entry and one active state.
    ir.model.curves.push(ir.model.curves[0].clone());
    let index = crate::index::ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 65536;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::walk_cycles(&ctx, &ir, &index, |finding| {
        findings.push(finding);
        Ok(false)
    })
    .unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        format!("malformed curve/surface reference cycle: {curve} -> {surface} -> {curve}")
    );
    drop(
        ctx.reserve_scoped(65536, "cycle graph storage released")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn cycle_dependencies_borrow_fixed_slots_in_declared_order() {
    let (ir, _, _) = cyclic_model();
    let definition = ir.model.procedural_curves[0].definition();
    let crate::geometry::ProceduralCurveDefinition::TolerantIntersection { construction, .. } =
        definition
    else {
        panic!("intersection fixture");
    };
    let dependencies = super::curve_dependencies(definition);
    for (slot, support) in dependencies[..2].iter().zip(construction.supports()) {
        assert!(std::ptr::eq(slot.unwrap(), support.as_str()));
    }
    assert_eq!(dependencies[2], None);
    let mut absent = definition.clone();
    let crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
        parameterization, ..
    } = &mut absent
    else {
        panic!("intersection fixture");
    };
    *parameterization = None;
    assert_eq!(super::curve_dependencies(&absent), [None; 3]);
    let definition = ir.model.procedural_surfaces[0].definition();
    let crate::geometry::ProceduralSurfaceDefinition::AxisRevolution(payload) = definition else {
        panic!("axis-revolution fixture");
    };
    let dependencies = super::surface_dependencies(definition);
    assert!(std::ptr::eq(
        dependencies[0].unwrap(),
        payload.directrix().as_str()
    ));
    assert_eq!(&dependencies[1..], &[None, None]);
}

#[test]
fn cycle_blend_fixed_slots_skip_absent_sides_before_the_original_cycle() {
    use crate::geometry::surface_payloads::BlendSurfacePayload;
    use crate::geometry::{
        BlendCrossSection, BlendRadiusLaw, CacheContract, ProceduralSurfaceDefinition,
        RevisionCacheForm, RevisionSurfaceParameterization, RollingBallConstruction,
        RollingBallRadiusSelector, RollingBallSide, RollingBallSupportSurface,
        VariableBlendSupportKind,
    };
    for (first_present, second_present) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let (mut ir, curve, support) = cyclic_model();
        let other = ir.model.surfaces[1].id.clone();
        let side = |surface: Option<crate::ids::SurfaceId>| RollingBallSide {
            support_kind: VariableBlendSupportKind::Surface,
            surface: surface.map(|surface| RollingBallSupportSurface {
                surface,
                parameter_ranges: [[None, None]; 2],
            }),
            curve: None,
            pcurve: None,
            location: crate::math::Point3::new(0.0, 0.0, 0.0),
            secondary_pcurve: None,
            extension: None,
        };
        let native = RollingBallConstruction {
            revision: crate::scalar::PositiveI64::new(1).unwrap(),
            sides: [
                side(first_present.then_some(other)),
                side(second_present.then_some(support.clone())),
            ],
            slice: curve.clone(),
            slice_range: [None, None],
            offsets: [0.0, 0.0],
            radius_selector: RollingBallRadiusSelector::None {},
            u_range: [None, None],
            v_range: [None, None],
            shape_prefix: 0,
            parameters: [0.0, 0.0],
            tail: 0,
            cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization::default()),
            discontinuities: std::array::from_fn(|_| Vec::new()),
            tail_flag: false,
            third: None,
            tail_extensions: [0; 3],
        };
        let definition = ProceduralSurfaceDefinition::Blend(
            BlendSurfacePayload::try_new(
                [None, None],
                None,
                BlendRadiusLaw::constant(1.0).unwrap(),
                BlendCrossSection::Circular,
                CacheContract::from_form(Some(Box::new(native))),
            )
            .unwrap(),
        );
        let ProceduralSurfaceDefinition::Blend(payload) = &definition else {
            panic!("blend fixture");
        };
        let native = payload.native().unwrap();
        let slots = super::surface_dependencies(&definition);
        assert_eq!(slots[0].is_some(), first_present);
        assert_eq!(slots[1].is_some(), second_present);
        for (slot, side) in slots[..2].iter().zip(&native.sides) {
            if let Some(surface) = &side.surface {
                assert!(std::ptr::eq(slot.unwrap(), surface.surface.as_str()));
            }
        }
        assert!(std::ptr::eq(slots[2].unwrap(), native.slice.as_str()));
        ir.model.procedural_surfaces[0].edit_definition(|current| *current = definition);
        let expected = if second_present {
            format!("malformed curve/surface reference cycle: {support} -> {support}")
        } else {
            format!("malformed curve/surface reference cycle: {curve} -> {support} -> {curve}")
        };
        assert!(
            matches!(admit_evaluation_cycles(&cadmpeg_test_support::service_decode_context(),&ir), Err(CodecError::Malformed(message)) if message == expected)
        );
    }
}
