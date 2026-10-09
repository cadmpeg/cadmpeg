// SPDX-License-Identifier: Apache-2.0
//! Morph projection refuses the next actual item, without its unused suffix.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, NonZeroReal};
use super::super::{Cage, Control, Localizer, LocalizerKind, Morph, NurbsSurface, Uuid};

fn assert_prefix(operation: &str, run: impl FnOnce(&DecodeContext<'_>) -> Result<(), CodecError>) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The named probe selects the exact boundary after earlier operations.
    policy.limits.max_work_units = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
    let CodecError::ResourceLimit(refusal) = run(&ctx).expect_err("first actual visit refuses")
        else { panic!("morph visit refusal"); };
    drop(probe);
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, operation);
    assert_eq!(refusal.additional, 1);
    assert_eq!(refusal.used, refusal.limit);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn morph_point_projection_refuses_only_first_visit() {
    let points = vec![FinitePoint3::ZERO; 1024];
    let mut text = String::new();
    assert_prefix("Rhino morph point projection", |ctx| {
        super::super::append_points(ctx, &mut text, &points, |point| point.get())
    });
    assert!(text.is_empty());
}

#[test]
fn morph_empty_point_projection_preserves_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "original morph refusal").unwrap_err()
        else { panic!("original refusal"); };
    let points: [FinitePoint3; 0] = [];
    assert!(matches!(super::super::append_points(&ctx, &mut String::new(), &points, |point| point.get()),
        Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

fn surface(rational: bool) -> NurbsSurface {
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let grid = vec![vec![FinitePoint3::ZERO; 2]; 2];
    let weights = rational.then(|| vec![vec![NonZeroReal::new(1.0).unwrap(); 2]; 2]);
    NurbsSurface::from_checked_lanes(&cadmpeg_test_support::service_decode_context(),
        axis(), axis(), NurbsSurfaceLanes::new(grid, weights), false).unwrap().unwrap()
}

#[test]
fn morph_surface_rows_refuse_only_first_visit() {
    for rational in [false, true] {
        let surface = surface(rational);
        assert_prefix("Rhino morph surface rows", |ctx| {
            super::super::surface_properties(ctx, "surface", &surface, &mut std::collections::BTreeMap::new())
        });
    }
}

#[test]
fn morph_surface_weights_refuse_only_first_row_and_pole_visits() {
    let surface = surface(true);
    for operation in ["Rhino morph surface weight rows", "Rhino morph surface weights"] {
        assert_prefix(operation, |ctx| {
            super::super::surface_properties(ctx, "surface", &surface, &mut std::collections::BTreeMap::new())
        });
    }
}

fn cage() -> Cage {
    Cage { source_range: 0..0, dimension: 3, orders: [2; 3], counts: [2; 3],
        knots: std::array::from_fn(|_| vec![FiniteReal::ZERO, FiniteReal::ONE]),
        control_points: vec![vec![FiniteReal::ZERO; 3]; 8], weights: None }
}

#[test]
fn morph_cage_coordinates_refuse_only_first_point_and_coordinate_visits() {
    let cage = cage();
    for operation in ["Rhino morph cage points", "Rhino morph cage coordinates"] {
        assert_prefix(operation, |ctx| {
            super::super::cage_properties(ctx, "cage", &cage, &mut std::collections::BTreeMap::new())
        });
    }
}

#[test]
fn morph_localizer_and_captive_projection_refuse_only_first_visit() {
    for localizers in [false, true] {
        let morph = Morph { source_range: 0..0,
            control: Control::Cage { start_transform: cadmpeg_ir::units::FiniteVector::new([0.0; 16]).unwrap(), end: cage() },
            captive_ids: if localizers { Vec::new() } else { vec![Uuid::nil(); 1024] },
            localizers: if localizers { vec![Localizer { kind: LocalizerKind::NONE,
                point: cadmpeg_ir::units::FiniteVector::new([0.0; 3]).unwrap(),
                vector: cadmpeg_ir::units::FiniteVector::new([0.0; 3]).unwrap(),
                interval: cadmpeg_ir::units::FiniteVector::new([0.0; 2]).unwrap(), curve: None, surface: None }; 1024] } else { Vec::new() },
            tolerance: NonNegativeReal::ZERO, quick_preview: false, preserve_structure: false };
        assert_prefix("Rhino project traversal", |ctx| {
            super::super::project(ctx, &morph, "fixture", None, "native".into(), |_| Ok(None)).map(drop)
        });
    }
}
