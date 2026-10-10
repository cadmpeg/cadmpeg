// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;


#[test]
fn inline_envelope_search_stops_at_second_close() {
    use crate::surface::{
        SurfaceBodyBoundary, SurfaceKind, SurfaceParameterCarrier, SurfaceParameterRecord,
    };
    let record = |body| SurfaceParameterRecord {
        surface_id: 7,
        body,
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: SurfaceParameterCarrier::Unresolved(SurfaceKind::Cylinder),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let short = record(vec![0x12, 0xe3, 0xe3]);
    let mut long = short.clone();
    long.body.extend([0; 512]);
    let refusal = |record: &SurfaceParameterRecord| {
        crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo inline envelope delimiter search",
            |ctx| record.has_inline_non_plane_envelope_checked(ctx),
        )
    };
    let (CodecError::ResourceLimit(short_limit), CodecError::ResourceLimit(long_limit)) =
        (refusal(&short), refusal(&long))
    else {
        panic!("work refusals");
    };
    assert_eq!(short_limit.used, long_limit.used);
    assert_eq!(short_limit.additional, long_limit.additional);
    for record in [short, long] {
        assert!(!crate::decode::with_test_decode_ctx(
            |ctx| record.has_inline_non_plane_envelope_checked(ctx)
        )
        .expect("delimiter search admitted"));
    }
}

#[test]
fn terminal_corner_search_stops_at_first_gap() {
    use crate::surface::{
        SurfaceBodyBoundary, SurfaceKind, SurfaceParameterCarrier, SurfaceParameterRecord,
        SurfaceParameterScalar, SurfaceParameterScalarFrame,
    };
    let record = |count| SurfaceParameterRecord {
        surface_id: 7,
        body: vec![0x18; count],
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: vec![SurfaceParameterScalarFrame {
            offset: 0,
            slots: (0..count)
                .map(|offset| SurfaceParameterScalar {
                    value: Some(0.0),
                    raw: vec![0x18],
                    offset: offset + 1,
                })
                .collect(),
        }],
        carrier: SurfaceParameterCarrier::Unresolved(SurfaceKind::Cylinder),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let short = record(6);
    let long = record(512);
    let refusal = |record: &SurfaceParameterRecord| {
        crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo terminal corner frame traversal",
            |ctx| record.type24_terminal_corner_envelope_checked(ctx),
        )
    };
    let (CodecError::ResourceLimit(short_limit), CodecError::ResourceLimit(long_limit)) =
        (refusal(&short), refusal(&long))
    else {
        panic!("work refusals");
    };
    assert_eq!(short_limit.used, long_limit.used);
    assert_eq!(short_limit.additional, long_limit.additional);
    for record in [short, long] {
        assert!(crate::decode::with_test_decode_ctx(
            |ctx| record.type24_terminal_corner_envelope_checked(ctx)
        )
        .expect("corner traversal admitted")
        .is_none());
    }
}

#[test]
fn torus_radius_search_stops_at_second_marker() {
    let short = [0x18, 0x0d, 0x18, 0x0d];
    let mut long = short.to_vec();
    long.extend([0; 512]);
    let refusal = |body: &[u8]| {
        crate::test_support::last_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            "creo torus radius marker search",
            |ctx| crate::surface::torus_radius_override_layout(ctx, body),
        )
    };
    let (CodecError::ResourceLimit(short_limit), CodecError::ResourceLimit(long_limit)) =
        (refusal(&short), refusal(&long))
    else {
        panic!("work refusals");
    };
    assert_eq!(short_limit.used, long_limit.used);
    assert_eq!(short_limit.additional, long_limit.additional);
    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        crate::surface::torus_radius_override_layout(ctx, &long)
    })
    .expect("marker search admitted")
    .is_none());
}
