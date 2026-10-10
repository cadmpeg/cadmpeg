// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::surface::{BoundaryType, SurfaceBodyBoundary, SurfaceParameterCarrier,
    SurfaceParameterOpaqueSpan, SurfaceParameterScalar, SurfaceParameterScalarFrame, SurfaceRow};

fn active_parameter(preceding: &[&[Option<f64>]], terminal_length: f64)
    -> (SurfaceRow, crate::surface::SurfaceParameterRecord)
{
    let row = SurfaceRow { id: 8, kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 3, reversed: false, boundary_type: BoundaryType::Code01,
        next_surface: 0, offset: 0 };
    let mut body = Vec::new();
    let mut frames = Vec::new();
    let mut tokens = Vec::new();
    let mut opaque_spans = Vec::new();
    let terminal = [Some(terminal_length), Some(-13.0), Some(-4.0), Some(0.0),
        Some(-11.0), Some(4.0), Some(1.0)];
    for values in preceding.iter().copied().chain(std::iter::once(terminal.as_slice())) {
        let offset = body.len();
        let mut slots = Vec::new();
        for &value in values {
            let raw = match value {
                None => vec![0x45, 0, 0, 0, 0, 0, 0],
                Some(0.0) => vec![0x0f],
                Some(1.0) => vec![0xe4],
                Some(value) => ieee8(value),
            };
            let slot = SurfaceParameterScalar { value, offset: body.len(), raw };
            body.extend_from_slice(&slot.raw);
            tokens.push(slot.clone());
            slots.push(slot);
        }
        frames.push(SurfaceParameterScalarFrame { offset, slots });
        opaque_spans.push(SurfaceParameterOpaqueSpan { offset: body.len(), raw: vec![0xc0] });
        body.push(0xc0);
    }
    body.pop();
    opaque_spans.pop();
    let parameter = crate::surface::SurfaceParameterRecord { surface_id: row.id,
        body, scalar_tokens: tokens, opaque_spans, scalar_frames: frames,
        carrier: SurfaceParameterCarrier::Unresolved(row.kind),
        boundary: SurfaceBodyBoundary::SectionEnd, offset: row.offset, body_offset: 6 };
    (row, parameter)
}

#[test]
fn active_datum_fixed_terminal_frame_is_free_and_keeps_original_refusal() {
    let (row, parameter) = active_parameter(&[], 8.0);
    let cache = crate::scalar::ScalarCache::default();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let frame = super::super::active_cylinder_frame(&ctx, &row, &parameter)
        .expect("fixed terminal frame").expect("complete active cylinder");
    assert_eq!(frame.frame().origin(), [-12.0, 4.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, -1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius().get(), 1.0);
    assert_eq!(frame.length().map(PositiveLength::get), Some(8.0));
    assert_eq!(super::super::positional_plane(&ctx, &[], &row, 0, &cache)
        .expect("fixed invalid plane prefix"), None);
    let original = ctx.charge_work_limit(1, "seed fixed datum refusal")
        .expect_err("zero work cap");
    assert_eq!((original.used, original.additional), (0, 1));
    assert!(matches!(super::super::active_cylinder_frame(&ctx, &row, &parameter),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert!(matches!(super::super::positional_plane(&ctx, &[], &row, 0, &cache),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn active_datum_admits_present_preceding_frames_and_slots_without_terminal_fee() {
    let split = [None, Some(0.0), Some(8.0)];
    let conflict = [Some(-8.0), Some(8.0), Some(42.0)];
    for (values, terminal, accepted) in [
        (split.as_slice(), 0.0, true), (conflict.as_slice(), 8.0, false),
    ] {
        let (row, parameter) = active_parameter(&[values], terminal);
        let frame = super::work_output(|ctx| super::super::active_cylinder_frame(ctx, &row, &parameter));
        assert_eq!(frame.is_some(), accepted);
        if let Some(frame) = frame {
            assert_eq!(frame.frame().origin(), [-12.0, 4.0, 0.0]);
            assert_eq!(frame.frame().axis(), [0.0, -1.0, 0.0]);
            assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
            assert_eq!(frame.radius().get(), 1.0);
            assert_eq!(frame.length().map(PositiveLength::get), Some(8.0));
        }
        for operation in ["creo active datum cylinder scalar frames", "creo active datum cylinder scalar slots"] {
            let error = crate::test_support::last_refusal_at(&[], ResourceDimension::WorkUnits, operation,
                |ctx| super::super::active_cylinder_frame(ctx, &row, &parameter));
            assert!(matches!(error, CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::WorkUnits && refusal.operation == operation));
        }
    }
}

#[test]
fn named_datum_identity_searches_admit_prefixes_and_stop_at_first_missing_identity() {
    let marker = b"outline\0\xf9\x02\x03";
    for (prefix, geometry_present) in [
        (b"missing geometry".as_slice(), false),
        (b"\xe0\x01geom_id\0\x07padding".as_slice(), true),
    ] {
        let mut data = prefix.to_vec();
        data.extend_from_slice(marker);
        assert_eq!(super::work_output(|ctx| named_plane(ctx, &data)), None);
        let operation = if geometry_present { "creo named datum feature identity" }
            else { "creo named datum geometry identity" };
        let error = crate::test_support::last_refusal_at(&[], ResourceDimension::WorkUnits, operation,
            |ctx| named_plane(ctx, &data));
        assert!(matches!(error, CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::WorkUnits && refusal.operation == operation));
    }
}

#[test]
fn datum_plane_row_traversal_keeps_other_surface_families_out_of_the_plane_roster() {
    let mut data = b"srf_array\0\xf8\x02".to_vec();
    data.extend([9, 0x24, 3, 1, 1, 0]);
    data.extend([4, 0x22, 1, 1, 1, 0]);
    data.extend([0x0f; 4]);
    for value in [2.0, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 { data.push(0x0f); } else { data.extend(ieee8(value)); }
    }
    let expected = DatumPlaneRecord::new(4, 1, DatumPlane::new(Axis::Y, 0.0)
        .expect("plane"), 0.0, [[Some(2.0), Some(3.0)], [Some(-2.0), Some(-3.0)]],
        b"srf_array\0\xf8\x02".len() + 6).expect("matching outline");
    assert_eq!(planes_ok(&data), vec![expected]);
}
