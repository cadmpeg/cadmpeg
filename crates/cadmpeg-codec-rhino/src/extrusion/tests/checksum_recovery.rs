// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::chunks::chunk_at;
use crate::curves::GeometryError;
use crate::loss::{RhinoDiagnostic, RhinoLossCode};
use crate::mesh::{MeshBudget, MeshExpand};

use super::{ExtrusionFormat, MillimeterScale, CHUNKS};

fn bad_profile_uuid_crc() -> Vec<u8> {
    let mut profile = super::polyline_wrapper(false, true);
    let wrapper = chunk_at(&profile, 0, profile.len(), CHUNKS, false).expect("profile wrapper");
    let uuid = chunk_at(
        &profile,
        wrapper.body().start,
        wrapper.body().end,
        CHUNKS,
        true,
    )
    .expect("profile class UUID");
    assert_eq!(uuid.typecode, 0x0002_fffb);
    profile[uuid.body().end] ^= 1;
    super::payload_with_profile(2, [false, false], None, profile)
}

fn format() -> ExtrusionFormat {
    ExtrusionFormat {
        archive: CHUNKS,
        writer_version: None,
        scale: MillimeterScale::IDENTITY,
    }
}

#[test]
fn profile_uuid_crc_warning_preserves_analytic_extrusion() {
    let clean = super::payload(2, [false, false], None);
    let damaged = bad_profile_uuid_crc();
    let run = |bytes: &[u8]| {
        super::decode(
            bytes,
            0..bytes.len(),
            CHUNKS,
            None,
            MillimeterScale::IDENTITY,
            &mut MeshBudget::new(),
        )
        .expect("bounded extrusion remains recoverable")
    };
    let expected = run(&clean);
    let actual = run(&damaged);
    assert_eq!(actual.boundaries.len(), 1);
    assert_eq!(actual.caps, expected.caps);
    assert_eq!(actual.cap_origins, expected.cap_origins);
    assert_eq!(actual.cap_normals, expected.cap_normals);
    assert_eq!(actual.cap_u_axes, expected.cap_u_axes);
    assert_eq!(actual.direction, expected.direction);
    let first = &actual.boundaries[0];
    let reference = &expected.boundaries[0];
    assert_eq!(first.start_nurbs, reference.start_nurbs);
    assert_eq!(first.end_nurbs, reference.end_nurbs);
    for (curve, original) in [
        (&first.start_pcurve, &reference.start_pcurve),
        (&first.end_pcurve, &reference.end_pcurve),
    ] {
        assert_eq!(curve.degree, original.degree);
        assert_eq!(curve.knots, original.knots);
        assert_eq!(curve.control_points, original.control_points);
        assert_eq!(curve.weights, original.weights);
        assert_eq!(curve.periodic, original.periodic);
    }
    assert_eq!(first.lateral, reference.lateral);
    assert!(reference.start_curve.warnings().is_empty());
    let diagnostics = first.start_curve.warnings();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, Some(RhinoLossCode::IntegrityFailure));
    assert!(diagnostics[0].message.starts_with("CRC mismatch at offset "));
    assert!(diagnostics[0].message.contains("typecode 0x2fffb:"));
    assert!(actual.warnings.is_empty());
    assert!(actual.meshes.is_empty());
}

#[test]
fn profile_uuid_crc_warning_refuses_before_diagnostic_slot() {
    let bytes = bad_profile_uuid_crc();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root view");
    let error = super::super::decode(
        MeshExpand::new(&ctx, root),
        &bytes,
        0..bytes.len(),
        format(),
        &[],
        &mut MeshBudget::new(),
    )
    .expect_err("checksum warning needs one diagnostic slot");
    let GeometryError::Codec(CodecError::ResourceLimit(first)) = error else {
        panic!("original resource refusal: {error:?}");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!((first.used, first.additional), (0, 1));
    assert_eq!(first.operation, "Rhino diagnostics");
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first
    ));
}

#[test]
fn profile_uuid_crc_warning_refuses_before_message_storage() {
    let bytes = bad_profile_uuid_crc();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Core amortized growth reserves four slots for this non-byte record.
    let diagnostic_backing = 4 * std::mem::size_of::<RhinoDiagnostic>();
    policy.limits.max_materialized_bytes = u64::try_from(diagnostic_backing).expect("backing size");
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root view");
    let error = super::super::decode(
        MeshExpand::new(&ctx, root),
        &bytes,
        0..bytes.len(),
        format(),
        &[],
        &mut MeshBudget::new(),
    )
    .expect_err("diagnostic backing leaves no room for its message");
    let GeometryError::Codec(CodecError::ResourceLimit(first)) = error else {
        panic!("original resource refusal: {error:?}");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(
        first.used,
        u64::try_from(diagnostic_backing).expect("backing size")
    );
    assert!(first.additional > 0);
    assert_eq!(first.operation, "Rhino diagnostic message");
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first
    ));
}


#[test]
fn profile_uuid_crc_warning_copy_admits_retained_message_storage() {
    let bytes = bad_profile_uuid_crc();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::RetainedBytes,
        "Rhino diagnostic copy text", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let result = super::super::decode(MeshExpand::new(&ctx, root), &bytes,
                0..bytes.len(), format(), &[], &mut MeshBudget::new());
            if let Err(GeometryError::Codec(CodecError::ResourceLimit(limit))) = &result {
                assert!(limit.additional > 0);
                assert_eq!(ctx.resource_refusal(), Some(*limit));
            }
            result.map(|_| ()).map_err(|error| match error {
                GeometryError::Codec(error) => error, error => panic!("recoverable profile CRC: {error:?}"),
            })
        });
}
