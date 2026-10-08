// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn degree_elevation_rejects_zero_active_domain_before_allocating() {
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0; 4],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("fixture admission")
    .expect("finite ordered zero-domain knots");
    let error = with_collection_limit(0, |ctx| super::super::elevate_to_degree(ctx, &curve, 1, 7))
        .expect_err("a zero-domain segment has no Bezier span");
    assert!(matches!(error,
        GeometryError::Malformed(FramingError::Structural { offset: 7, ref message })
        if message == "polycurve segment has no nonempty span"));
}

fn chunk(typecode: u32, body: &[u8], protected: bool) -> Vec<u8> {
    let mut body = body.to_vec();
    if protected {
        let crc = crate::chunks::crc16(&cadmpeg_test_support::service_decode_context(), 0, &body)
            .expect("fixture checksum admission");
        body.extend(crc.to_le_bytes());
    }
    let mut bytes = typecode.to_le_bytes().to_vec();
    bytes.extend(
        i32::try_from(body.len())
            .expect("fixture length")
            .to_le_bytes(),
    );
    bytes.extend(body);
    bytes
}

fn face_archive(first_end: f64) -> Vec<u8> {
    let corners = [[0.0_f64, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let mut boundary = 4_i32.to_le_bytes().to_vec();
    boundary.extend(0_i32.to_le_bytes());
    boundary.extend(
        [0.0_f64, 0.0, 1.0, 1.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    for index in 0..4 {
        let from = corners[index];
        let to = corners[(index + 1) % 4];
        let bounds = [from[0], from[1], to[0], to[1]];
        let mut spline = vec![2, 0, 2];
        spline.extend(2_u16.to_le_bytes());
        spline.extend([0, 0]);
        spline.extend(bounds.into_iter().flat_map(f64::to_le_bytes));
        spline.extend(0.0_f64.to_le_bytes());
        spline.extend((if index == 0 { first_end } else { 1.0 }).to_le_bytes());
        spline.extend(bounds.into_iter().flat_map(f64::to_le_bytes));
        let spline = chunk(0x0001_0009, &chunk(0x0001_0109, &spline, true), true);
        let mut curve = vec![2, 0];
        curve.extend(1_u16.to_le_bytes());
        curve.extend(bounds.into_iter().flat_map(f64::to_le_bytes));
        curve.extend(spline);
        let curve = chunk(0x0001_0008, &chunk(0x0001_0108, &curve, true), true);
        let mut trim = vec![1];
        trim.extend([0_i32; 3].into_iter().flat_map(i32::to_le_bytes));
        trim.extend([0.001_f64; 2].into_iter().flat_map(f64::to_le_bytes));
        trim.extend(curve);
        let bounds_3d = [from[0], from[1], 0.0, to[0], to[1], 0.0];
        let mut spline = vec![3, 0, 2];
        spline.extend(2_u16.to_le_bytes());
        spline.extend([0, 0]);
        spline.extend(bounds_3d.into_iter().flat_map(f64::to_le_bytes));
        spline.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
        spline.extend(bounds_3d.into_iter().flat_map(f64::to_le_bytes));
        let spline = chunk(0x0001_0009, &chunk(0x0001_0109, &spline, true), true);
        let mut curve = vec![3, 0];
        curve.extend(1_u16.to_le_bytes());
        curve.extend(bounds_3d.into_iter().flat_map(f64::to_le_bytes));
        curve.extend(spline);
        trim.extend(chunk(0x0001_0008, &chunk(0x0001_0108, &curve, true), true));
        boundary.extend(chunk(0x0001_0006, &chunk(0x0001_0106, &trim, true), true));
    }
    let boundary = chunk(0x0001_0005, &chunk(0x0001_0105, &boundary, true), true);
    let mut surface = vec![3, 0, 1, 1];
    surface.extend([1_u16; 2].into_iter().flat_map(u16::to_le_bytes));
    surface.extend([0; 6]);
    surface.extend(
        [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    surface.extend(
        [0.0_f64, 1.0, 0.0, 1.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    for point in [
        [0.0_f64, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
    ] {
        surface.extend(point.into_iter().flat_map(f64::to_le_bytes));
    }
    let surface = chunk(0x0001_0007, &chunk(0x0001_0107, &surface, true), true);
    let mut face = [0_i32, 0, 3]
        .into_iter()
        .flat_map(i32::to_le_bytes)
        .collect::<Vec<_>>();
    face.extend(
        [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    face.extend(0_i32.to_le_bytes());
    face.extend(surface);
    face.extend(boundary);
    let mut archive = crate::chunks::MAGIC.to_vec();
    archive.extend(*b"       1");
    archive.extend(chunk(1, b"trim domain", false));
    archive.extend(chunk(0x0001_0004, &chunk(0x0001_0104, &face, true), true));
    archive
}

#[test]
fn v1_trim_zero_domain_returns_geometry_diagnostic_and_retains_source() {
    use cadmpeg_test_support::EditableDecodeResult;

    for end in [1.0, 0.0] {
        let bytes = face_archive(end);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("fixture root");
        let result = EditableDecodeResult::from(crate::decode::seal_for_test(
            crate::legacy::decode_v1(&ctx, &bytes).expect("V1 face decode returns without panic"),
            false,
        ));
        if end == 1.0 {
            assert_eq!(result.ir().model.bodies.len(), 1, "{:?}", result.report());
            assert_eq!(result.ir().model.pcurves.len(), 4);
        } else {
            assert_eq!(result.ir().model.entity_count(), 0);
            assert!(result
                .report()
                .notes
                .iter()
                .any(|note| note.contains("polycurve segment has no nonempty span")));
            let comment =
                crate::chunks::chunk_at(&bytes, 32, bytes.len(), ArchiveVersion::V1, false)
                    .expect("fixture comment");
            assert!(result
                .source_fidelity()
                .retained_records()
                .values()
                .any(|record| record.data() == Some(&bytes[comment.next_offset()..])));
        }
    }
}
