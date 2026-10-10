// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{CurveGeometry, RollingBallSide, RollingBallSideExtension,
    RollingBallSupportCurve, RollingBallSupportSurface, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SurfaceGeometry, VariableBlendSupportKind};
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::{Point3, Vector3};
use crate::brep::AsmBrep;

fn side(surface: bool, curve: bool) -> RollingBallSide<SurfaceGeometry, CurveGeometry, PcurveNurbs> {
    RollingBallSide {
        support_kind: if surface { VariableBlendSupportKind::Surface }
            else if curve { VariableBlendSupportKind::Curve }
            else { VariableBlendSupportKind::ZeroCurve },
        surface: surface.then(|| RollingBallSupportSurface {
            surface: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)).unwrap())),
            parameter_ranges: [[Some(0.0), Some(1.0)], [None, Some(2.0)]],
        }),
        curve: curve.then(|| RollingBallSupportCurve {
            curve: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0)).unwrap())),
            parameter_range: [Some(0.0), Some(1.0)],
        }),
        pcurve: None, location: Point3::new(1.0, 2.0, 3.0), secondary_pcurve: None,
        extension: Some(RollingBallSideExtension { value: 41, pcurve: None }),
    }
}

fn present(surface: bool, curve: bool) {
    let input = side(surface, curve);
    let expected = input.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let calls = Cell::new(0);
    let mut out = AsmBrep::default();
    let source_index = 7_i64;
    let mapped = super::super::emit_rolling_ball_side(&ctx, &mut out, crate::asm_format!("sat"),
        || { calls.set(calls.get() + 1); Ok(crate::ids::brep_key!(source_index, ":native_side0")) }, input).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(mapped.support_kind, expected.support_kind);
    assert_eq!(mapped.location, expected.location);
    assert!(mapped.pcurve.is_none() && mapped.secondary_pcurve.is_none());
    assert!(matches!(mapped.extension, Some(RollingBallSideExtension { value: 41, pcurve: None })));
    assert_eq!(out.surfaces.len(), usize::from(surface));
    assert_eq!(out.curves.len(), usize::from(curve));
    match (mapped.surface, expected.surface) {
        (Some(actual), Some(expected)) => {
            assert_eq!(actual.surface.as_str(), "sat:brep:procedural_surface#7:native_side0:surface");
            assert_eq!(actual.parameter_ranges, expected.parameter_ranges);
            assert_eq!(out.surfaces[0].id, actual.surface);
            assert_eq!(out.surfaces[0].geometry, expected.surface);
            assert!(out.surfaces[0].source_object.is_none());
        }
        (None, None) => {}
        _ => panic!("support surface presence changed"),
    }
    match (mapped.curve, expected.curve) {
        (Some(actual), Some(expected)) => {
            assert_eq!(actual.curve.as_str(), "sat:brep:procedural_surface#7:native_side0:curve");
            assert_eq!(actual.parameter_range, expected.parameter_range);
            assert_eq!(out.curves[0].id, actual.curve);
            assert_eq!(out.curves[0].geometry, expected.curve);
            assert!(out.curves[0].source_object.is_none());
        }
        (None, None) => {}
        _ => panic!("support curve presence changed"),
    }
    ctx.finish_session().unwrap();
}

#[test]
fn rolling_surface_calls_identity_once_and_keeps_geometry() { present(true, false); }

#[test]
fn rolling_curve_calls_identity_once_and_keeps_geometry() { present(false, true); }

#[test]
fn rolling_surface_and_curve_share_one_identity_factory() { present(true, true); }

#[test]
fn rolling_present_sides_skip_factory_after_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let Some(first) = original else { return; };
        for (surface, curve) in [(true, false), (false, true), (true, true)] {
            let mut out = AsmBrep::default();
            assert!(matches!(super::super::emit_rolling_ball_side(ctx, &mut out,
                crate::asm_format!("sat"), || panic!("original refusal precedes factory"),
                side(surface, curve)), Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(out.surfaces.is_empty() && out.curves.is_empty());
        }
    });
}

#[test]
fn rolling_factory_refusal_precedes_both_output_arenas() {
    for (surface, curve) in [(true, false), (false, true), (true, true)] {
        let input = side(surface, curve);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let calls = Cell::new(0);
        let mut out = AsmBrep::default();
        let result = super::super::emit_rolling_ball_side(&ctx, &mut out, crate::asm_format!("sat"),
            || {
                calls.set(calls.get() + 1);
                ctx.charge_work(1, "test original rolling factory refusal")?;
                panic!("factory refuses before key exists")
            }, input);
        let Err(CodecError::ResourceLimit(first)) = result else { panic!("original factory refusal"); };
        assert_eq!(calls.get(), 1);
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "test original rolling factory refusal");
        assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
        for _ in 0..64 {
            for (surface, curve) in [(false, false), (true, false), (false, true), (true, true)] {
                assert!(matches!(super::super::emit_rolling_ball_side(&ctx, &mut out,
                    crate::asm_format!("sat"), || panic!("fused factory cannot run"),
                    side(surface, curve)), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
        assert!(out.surfaces.is_empty() && out.curves.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

fn surface_prefix_copy_bound(refuses: Option<ResourceDimension>) {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::IdentityKey;
    const EXPECTED_ID: &str = "sat:brep:procedural_surface#7:native_side0:surface";
    let copy = u64::try_from(EXPECTED_ID.len()).unwrap();
    let slots = if std::mem::size_of::<Surface>() <= 1024 { 4 } else { 1 };
    let backing = u64::try_from(slots * std::mem::size_of::<Surface>()).unwrap();
    let prefix = IdentityKey::try_new("7:native_side0".to_owned()).unwrap();
    let input = side(true, false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 1;
    policy.limits.max_work_units = copy - u64::from(refuses == Some(ResourceDimension::WorkUnits));
    policy.limits.max_retained_bytes = backing + copy
        - u64::from(refuses == Some(ResourceDimension::RetainedBytes));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    let mapped = super::super::emit_rolling_ball_side(&ctx, &mut out,
        crate::asm_format!("sat"), || Ok(prefix), input);
    let Some(dimension) = refuses else {
        let mapped = mapped.unwrap();
        assert_eq!(mapped.surface.unwrap().surface.as_str(), EXPECTED_ID);
        assert!(mapped.curve.is_none());
        assert_eq!(out.surfaces.len(), 1);
        assert_eq!(out.surfaces[0].id.as_str(), EXPECTED_ID);
        assert!(out.curves.is_empty());
        ctx.finish_session().unwrap();
        return;
    };
    let first = match mapped {
        Err(CodecError::ResourceLimit(first)) => first,
        _ => panic!("one byte below the emitted identity copy must refuse"),
    };
    assert_eq!(first.dimension, dimension);
    assert_eq!(first.operation, "ASM emitted identity copy");
    let (limit, used) = if dimension == ResourceDimension::WorkUnits {
        (copy - 1, 0)
    } else {
        (backing + copy - 1, backing)
    };
    assert_eq!((first.limit, first.used, first.additional), (limit, used, copy));
    assert!(out.surfaces.is_empty() && out.curves.is_empty());
    for _ in 0..64 {
        for (surface, curve) in [(false, false), (true, false), (false, true), (true, true)] {
            assert!(matches!(super::super::emit_rolling_ball_side(&ctx, &mut out,
                crate::asm_format!("sat"), || panic!("original copy refusal precedes factory"),
                side(surface, curve)), Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(out.surfaces.is_empty() && out.curves.is_empty());
        }
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn rolling_surface_moves_prefix_under_exact_copy_limits() {
    surface_prefix_copy_bound(None);
}

#[test]
fn rolling_surface_prefix_move_preserves_copy_refusal() {
    surface_prefix_copy_bound(Some(ResourceDimension::WorkUnits));
    surface_prefix_copy_bound(Some(ResourceDimension::RetainedBytes));
}
