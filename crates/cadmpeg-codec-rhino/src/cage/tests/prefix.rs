// SPDX-License-Identifier: Apache-2.0
use super::{decode, rational_cage_body, ANONYMOUS};
use crate::chunks::ArchiveVersion;
use crate::curves::GeometryError;
use crate::mesh::MeshExpand;
use crate::test_support::test_dump::crc_chunk;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn large_first_coordinate_body() -> Vec<u8> {
    let mut body = rational_cage_body();
    // This fixture has eight rational tuples, each with three coordinates and one weight.
    let tuple_bytes = 8 * 4 * std::mem::size_of::<f64>();
    body.truncate(body.len() - tuple_bytes);
    for index in 0..8 {
        let point = if index == 0 {
            [f64::MAX, f64::MAX, 0.0, 1.0]
        } else {
            [f64::from(index), 0.0, 0.0, 1.0]
        };
        for value in point {
            body.extend(value.to_le_bytes());
        }
    }
    body
}

#[test]
fn cage_first_coordinate_overflow_leaves_scaling_suffix_unvisited() {
    let bytes = crc_chunk(
        ArchiveVersion::V5,
        ANONYMOUS,
        &large_first_coordinate_body(),
    );
    let original = bytes.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Six knot reads, first control tuple, three stored coordinates, then first scaling visit.
    policy.limits.max_work_units = 6 + 1 + 3 + 1;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let error = decode(
        MeshExpand::new(&ctx, root),
        0..bytes.len(),
        crate::test_support::millimeter_scale(2.0),
        ArchiveVersion::V8,
    )
    .expect_err("first coordinate overflows");
    assert!(matches!(error, GeometryError::Malformed(_)));
    assert!(error
        .to_string()
        .contains("scaled NURBS cage coordinate is invalid"));
    assert!(ctx.resource_refusal().is_none());
    assert_eq!(bytes, original);
}

#[test]
fn cage_large_coordinates_decode_at_exact_visit_limit() {
    let bytes = crc_chunk(
        ArchiveVersion::V5,
        ANONYMOUS,
        &large_first_coordinate_body(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 6 + 8 * (1 + 3 + 3);
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let cage = decode(
        MeshExpand::new(&ctx, root),
        0..bytes.len(),
        crate::test_support::millimeter_scale(0.5),
        ArchiveVersion::V8,
    )
    .expect("finite scale control");
    assert_eq!(cage.control_points[0][0].get(), f64::MAX / 2.0);
    assert_eq!(cage.control_points[0][1].get(), f64::MAX / 2.0);
    assert_eq!(cage.control_points[7][0].get(), 3.5);
    ctx.finish_session().expect("no exhaustion probe");
}

#[test]
fn cage_header_truncation_preserves_required_scalar_width() {
    let bytes = crc_chunk(ArchiveVersion::V8, ANONYMOUS, &[]);
    let body_start = crate::chunks::chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false)
        .expect("fixture framing")
        .body()
        .start;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
    let error = decode(
        MeshExpand::new(&ctx, root),
        0..bytes.len(),
        crate::settings::MillimeterScale::IDENTITY,
        ArchiveVersion::V8,
    )
    .expect_err("major i32 is absent");
    assert!(
        matches!(error, GeometryError::Malformed(crate::chunks::FramingError::Truncated { offset, needed: 4 }) if offset == body_start)
    );
    ctx.finish_session()
        .expect("fixed header reads do not charge work");
}
