// SPDX-License-Identifier: Apache-2.0

use crate::scalar;
use crate::surface::decode_positional_cone_frame;
use crate::surface::prototype_cone_frame;
use crate::surface::terminal_cone_half_angle_layout;
use crate::surface::ApexConeHalfAngle;
use crate::surface::PositionalConeFrame;
use crate::surface::SurfaceNamedParameter;
use crate::surface::SurfaceNamedValue;
use crate::surface::SurfacePrototypeFamily;
use crate::surface::SurfacePrototypeRecord;

const EPS_FRAME_COMPONENT: f64 = 1.0e-12;

#[test]
fn positional_cone_frame_rejects_nonfinite_or_invalid_components() {
    let valid = PositionalConeFrame::new(
        [0.0, 1.0, 2.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        ApexConeHalfAngle::new(std::f64::consts::FRAC_PI_4).expect("apex cone half angle"),
    )
    .expect("valid positional cone frame");

    assert!(PositionalConeFrame::new(
        {
            let mut value = valid.frame().origin();
            value[1] = f64::NAN;
            value
        },
        valid.frame().axis(),
        valid.frame().ref_direction(),
        valid.half_angle
    )
    .is_none());

    assert!(ApexConeHalfAngle::new(0.0).is_none());

    assert!(PositionalConeFrame::new(
        valid.frame().origin(),
        [0.0, 2.0, 0.0],
        valid.frame().ref_direction(),
        valid.half_angle
    )
    .is_none());

    assert!(PositionalConeFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        [0.0, 1.0, 0.0],
        valid.half_angle
    )
    .is_none());

    assert!(ApexConeHalfAngle::new(std::f64::consts::FRAC_PI_2).is_none());
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
    assert_eq!(frame.frame().origin(), [37.01, 0.0, 0.0]);
    assert_eq!(frame.frame().axis(), [-1.0, -0.0, -0.0]);
    assert_eq!(frame.frame().ref_direction(), [-0.0, -0.0, -1.0]);
    assert!((frame.half_angle.get().get() - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12);

    let angle = terminal_cone_half_angle_layout(&body).expect("terminal half-angle");
    let mut local_system_body = vec![0xf9, 0x04, 0x03];
    local_system_body.extend_from_slice(&body[..angle.start]);
    let prototype = SurfacePrototypeRecord::new_for_test(
        SurfacePrototypeFamily::Cone,
        vec![
            SurfaceNamedParameter {
                name: "local_sys".to_string(),
                value: SurfaceNamedValue::Opaque(local_system_body.clone()),
                body: local_system_body,
                offset: 0,
                value_offset: 0,
            },
            SurfaceNamedParameter {
                name: "half_angle".to_string(),
                value: SurfaceNamedValue::ScalarSequence(vec![angle.value.get().get()]),
                body: body[angle.start..].to_vec(),
                offset: 0,
                value_offset: 0,
            },
        ],
        0,
    );
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
        assert_eq!(frame.frame().origin()[0], 0.0);
        assert!((frame.frame().origin()[1] + 19.389_817_409_565_175).abs() < EPS_FRAME_COMPONENT);
        assert_eq!(frame.frame().origin()[2], 0.0);
        assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
        assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
        assert!((frame.half_angle.get().get() - 0.636_540_466_818_335).abs() < 1.0e-12);
    }

    let mut inconsistent = unreferenced;
    inconsistent[43] = 0x98;
    assert!(decode_positional_cone_frame(&inconsistent, &scalar::ScalarCache::default()).is_none());
}

#[test]
fn cone_terminal_half_angle_has_one_fixed_start() {
    // Positive DICT 71 selects IEEE prefix 3f e6; six zero tail bytes
    // therefore state 0.6875 radians.
    const ANGLE: [u8; 7] = [0x71, 0, 0, 0, 0, 0, 0];
    for prefix_len in [0, 1, 7, 105, 106, 4096] {
        let mut body = vec![0x71; prefix_len];
        body.extend_from_slice(&ANGLE);
        let angle = terminal_cone_half_angle_layout(&body).expect("terminal angle");
        assert_eq!(angle.start, prefix_len);
        assert_eq!(angle.end, body.len());
        assert_eq!(angle.value.get().get(), 0.6875);
    }
    assert_eq!(terminal_cone_half_angle_layout(&[0xb7, 0, 0, 0, 0, 0, 0])
        .expect("alternate positive DICT head").value.get().get(), 0.625);
    // DICT 8b selects IEEE 40 00, which states 2.0 radians, above pi/2.
    for body in [&ANGLE[..6], &[0x00; 7], &[0x8b, 0, 0, 0, 0, 0, 0]] {
        assert!(terminal_cone_half_angle_layout(body).is_none());
    }
}

#[test]
fn cone_support_suffix_preserves_frame_after_unrelated_prefix() {
    // The first support is X; the third is Y. Their cross product is Z,
    // and the nonzero positive apex selects the negative Z axis.
    const SUPPORT: [u8; 9] = [0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f];
    let half_angle = ApexConeHalfAngle::new(0.6875).expect("finite positive angle");
    let cache = scalar::ScalarCache::default();
    for (apex, expected_apex) in [(&[0xe4][..], 1.0), (&[0x46, 0, 0, 0, 0, 0, 0, 0][..], 2.0)] {
        for reference_head in [0x19, 0x32] {
            for prefix_len in [0, 1, 105, 106, 4096] {
                let mut body = vec![0x19; prefix_len];
                body.extend_from_slice(&SUPPORT);
                body.extend_from_slice(apex);
                body.extend_from_slice(&[reference_head, 0, 0, 0, 0, 0, 0, 0]);
                body.extend_from_slice(&[0x32, 0x19, 0]);
                let frame = crate::surface::decode_support_apex_cone_frame(&body, half_angle, &cache)
                    .expect("complete support-apex suffix");
                assert_eq!(frame.frame().origin(), [0.0, 0.0, expected_apex]);
                assert_eq!(frame.frame().axis(), [0.0, 0.0, -1.0]);
                assert_eq!(frame.frame().ref_direction(), [0.0, -1.0, 0.0]);
                assert_eq!(frame.half_angle, half_angle);
                body[prefix_len + SUPPORT.len() + apex.len()] = 0xed;
                assert!(crate::surface::decode_support_apex_cone_frame(&body, half_angle, &cache).is_none());
            }
        }
    }
    for short_len in 0..11 {
        assert!(crate::surface::decode_support_apex_cone_frame(&[0x19; 11][..short_len], half_angle, &cache).is_none());
    }
}
