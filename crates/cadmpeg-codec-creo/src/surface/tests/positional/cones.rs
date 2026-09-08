// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn positional_cone_frame_rejects_nonfinite_or_invalid_components() {
    let valid = PositionalConeFrame::new(
        [0.0, 1.0, 2.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        std::f64::consts::FRAC_PI_4,
    )
    .expect("valid positional cone frame");

    assert!(PositionalConeFrame::new(
        {
            let mut value = valid.apex;
            value[1] = f64::NAN;
            value
        },
        valid.axis,
        valid.ref_direction,
        valid.half_angle
    )
    .is_none());

    assert!(PositionalConeFrame::new(valid.apex, valid.axis, valid.ref_direction, 0.0).is_none());

    assert!(PositionalConeFrame::new(
        valid.apex,
        [0.0, 2.0, 0.0],
        valid.ref_direction,
        valid.half_angle
    )
    .is_none());

    assert!(
        PositionalConeFrame::new(valid.apex, valid.axis, [0.0, 1.0, 0.0], valid.half_angle)
            .is_none()
    );

    assert!(PositionalConeFrame::new(
        valid.apex,
        valid.axis,
        valid.ref_direction,
        std::f64::consts::FRAC_PI_2
    )
    .is_none());
}

#[test]
fn positional_cone_frame_requires_complete_support_apex_and_angle() {
    let body = [
        197, 251, 126, 24, 209, 212, 112, 107, 81, 235, 133, 30, 184, 70, 125, 251, 126, 24, 209,
        212, 112, 123, 0, 68, 204, 99, 17, 228, 72, 66, 64, 192, 170, 175, 125, 232, 45, 177, 195,
        0, 68, 204, 99, 17, 220, 70, 66, 1, 69, 135, 177, 98, 82, 120, 170, 175, 125, 232, 45, 187,
        65, 200, 122, 225, 71, 174, 20, 128, 227, 24, 228, 15, 24, 15, 24, 16, 24, 228, 70, 66,
        129, 71, 174, 20, 122, 225, 25, 194, 145, 29, 33, 143, 32, 210, 52, 233, 0, 116, 33, 251,
        84, 68, 45, 5,
    ];
    let frame = decode_positional_cone_frame(&body, &scalar::ScalarCache::default())
        .expect("complete positional cone");
    assert_eq!(frame.apex, [37.01, 0.0, 0.0]);
    assert_eq!(frame.axis, [-1.0, -0.0, -0.0]);
    assert_eq!(frame.ref_direction, [-0.0, -0.0, -1.0]);
    assert!((frame.half_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12);

    let angle = terminal_cone_half_angle_layout(&body).expect("terminal half-angle");
    let mut local_system_body = vec![0xf9, 0x04, 0x03];
    local_system_body.extend_from_slice(&body[..angle.start]);
    let prototype = SurfacePrototypeRecord {
        family: SurfacePrototypeFamily::Cone,
        parameters: vec![
            SurfaceNamedParameter {
                name: "local_sys".to_string(),
                value: SurfaceNamedValue::Opaque(local_system_body.clone()),
                body: local_system_body,
                offset: 0,
                value_offset: 0,
            },
            SurfaceNamedParameter {
                name: "half_angle".to_string(),
                value: SurfaceNamedValue::ScalarSequence(vec![angle.value]),
                body: body[angle.start..].to_vec(),
                offset: 0,
                value_offset: 0,
            },
        ],
        offset: 0,
    };
    assert_eq!(prototype_cone_frame(&prototype), Some(frame));

    let mut incomplete = body.to_vec();
    incomplete.remove(86);
    assert!(decode_positional_cone_frame(&incomplete, &scalar::ScalarCache::default()).is_none());
}

#[test]
fn positional_cone_frame_decodes_complete_planar_envelopes() {
    let unreferenced = [
        21, 70, 34, 171, 89, 29, 204, 62, 140, 24, 70, 28, 153, 105, 188, 41, 208, 189, 71, 27,
        153, 70, 40, 122, 225, 71, 174, 20, 126, 24, 46, 27, 153, 70, 36, 28, 61, 7, 246, 190, 80,
        46, 27, 153,
    ];
    let referenced = [
        23, 70, 34, 171, 89, 29, 204, 62, 140, 21, 70, 28, 153, 105, 188, 41, 208, 189, 71, 27,
        153, 70, 40, 122, 225, 71, 174, 20, 126, 71, 27, 153, 46, 27, 153, 70, 36, 28, 61, 7, 246,
        190, 80, 25, 206, 113, 206, 177, 182, 81, 244, 247, 44,
    ];
    for body in [&unreferenced[..], &referenced[..]] {
        let frame = decode_positional_cone_frame(body, &scalar::ScalarCache::default())
            .expect("complete planar-envelope cone");
        assert_eq!(frame.apex[0], 0.0);
        assert!((frame.apex[1] + 19.389_817_409_565_175).abs() < 1.0e-12);
        assert_eq!(frame.apex[2], 0.0);
        assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert!((frame.half_angle - 0.636_540_466_818_335).abs() < 1.0e-12);
    }

    let mut inconsistent = unreferenced;
    inconsistent[43] = 0x98;
    assert!(decode_positional_cone_frame(&inconsistent, &scalar::ScalarCache::default()).is_none());
}
