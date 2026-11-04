use crate::layout::b5_plane_payload;
use cadmpeg_ir::topology::Sense;

use super::{assert_valid, decode};
use crate::test_support::test_container::{
    standard_catpart_from_streams, tetrahedron_topology_catpart,
};

fn complete_streams() -> (Vec<u8>, Vec<u8>) {
    let (mut main, mut surfaces) = crate::test_support::with_service_context(|ctx| {
        let scan = crate::container::scan_bytes(ctx, tetrahedron_topology_catpart())
            .expect("authored closed tetrahedron");
        let main = scan.main_data_stream().expect("main stream");
        let surfaces = &scan.brep.as_ref().expect("standard streams")[main.len()..];
        (main.to_vec(), surfaces.to_vec())
    });
    let packet = main
        .windows(4)
        .enumerate()
        .filter_map(|(at, bytes)| (bytes == [1, 0x44, 1, 0xff]).then_some(at))
        .nth(1)
        .expect("second tetrahedron face");
    main[packet + 1] = 0x4c;
    let normal = [1.0_f32, -1.0, 1.0]
        .into_iter()
        .map(|component| component / 3.0_f32.sqrt())
        .flat_map(f32::to_le_bytes);
    main.splice(packet + 8..packet + 8, normal);
    let mut plane = vec![2, 0, 0, 0, 2, 0, 0x33, 0x32];
    for value in [
        1.0_f32 / 3.0,
        -1.0 / 3.0,
        1.0 / 3.0,
        2.0,
        2.0,
        2.0,
        1.0 / 3.0,
        -1.0 / 3.0,
        1.0 / 3.0,
        4.0,
    ] {
        plane.extend_from_slice(&value.to_le_bytes());
    }
    plane.push(0xff);
    surfaces.splice(65..130, plane);
    (main, surfaces)
}

fn baseline_source() -> Vec<u8> {
    let (main, surfaces) = complete_streams();
    standard_catpart_from_streams(&main, &surfaces)
}

fn record(bytes: &mut Vec<u8>, class: u8, identity: u32, payload: &[u8]) {
    bytes.extend_from_slice(&[
        0xb5,
        0x03,
        class,
        u8::try_from(payload.len()).expect("short record"),
    ]);
    bytes.extend_from_slice(&identity.to_le_bytes());
    bytes.extend_from_slice(payload);
}

fn selected_plane_source(ambiguous: bool, invalid_sign: bool) -> Vec<u8> {
    let (mut main, surfaces) = complete_streams();
    record(&mut main, 0x65, 10, &[0x81, 0x8b, 0x05, 0x0d]);
    record(&mut main, 0x60, 11, &[0x81, 0x8c]);
    let sign = u8::from(!invalid_sign);
    record(
        &mut main,
        0x61,
        12,
        &[
            0x84, 0x81, 0x82, 0x83, 0x84, 0x84, 0x05, 0xff, 0xff, sign, 0, 0xff, 0xff, 0xff, 0xff,
            0x03,
        ],
    );
    for (face, carrier, terminal) in [(1, 30, 3), (2, 31, 5), (3, 32, 3), (4, 33, 3)] {
        record(
            &mut main,
            0x5f,
            face,
            &[0x82, 0x80 + carrier, 0xa8, terminal],
        );
    }
    let mut plane = vec![0; b5_plane_payload::LEN];
    plane[b5_plane_payload::LEAD] = 0x80;
    for (offset, values) in [
        (
            b5_plane_payload::ORIGIN,
            [1.0_f64 / 3.0, -1.0 / 3.0, 1.0 / 3.0],
        ),
        (
            b5_plane_payload::DIRECTION_U,
            [1.0_f64 / 2.0_f64.sqrt(), 1.0 / 2.0_f64.sqrt(), 0.0],
        ),
        (
            b5_plane_payload::DIRECTION_V,
            [
                1.0_f64 / 6.0_f64.sqrt(),
                -1.0 / 6.0_f64.sqrt(),
                -2.0 / 6.0_f64.sqrt(),
            ],
        ),
    ] {
        for (index, value) in values.into_iter().enumerate() {
            plane[offset + 8 * index..offset + 8 * (index + 1)]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for offset in [
        b5_plane_payload::FIXED_VALUES,
        b5_plane_payload::FIXED_VALUES + 8,
        b5_plane_payload::U_RANGE + 8,
        b5_plane_payload::V_RANGE + 8,
    ] {
        plane[offset..offset + 8].copy_from_slice(&1.0_f64.to_le_bytes());
    }
    record(&mut main, 0x27, 31, &plane);
    if ambiguous {
        record(&mut main, 0x65, 13, &[0x81, 0x8b, 0x05, 0x0d]);
    }
    standard_catpart_from_streams(&main, &surfaces)
}

#[test]
fn selected_shell_plane_parity_changes_only_the_oriented_face_normal() {
    let baseline = decode(baseline_source());
    let corrected = decode(selected_plane_source(false, false));
    assert_eq!(baseline.ir().model.faces.len(), 4);
    let mut expected = baseline.ir().model.clone();
    let face = expected
        .faces
        .iter_mut()
        .find(|face| face.id.as_str() == "catia:standard:face#1")
        .expect("plane face");
    assert_eq!(face.sense, Sense::Reversed);
    face.sense = Sense::Forward;
    assert_eq!(corrected.ir().model, expected);
    assert_valid(&corrected);
}

#[test]
fn ambiguous_or_malformed_shell_owner_preserves_the_existing_standard_model() {
    let baseline = decode(baseline_source());
    assert_eq!(baseline.ir().model.faces.len(), 4);
    for (ambiguous, invalid_sign) in [(true, false), (false, true)] {
        let result = decode(selected_plane_source(ambiguous, invalid_sign));
        assert_eq!(result.ir().model, baseline.ir().model);
        assert_valid(&result);
    }
}
