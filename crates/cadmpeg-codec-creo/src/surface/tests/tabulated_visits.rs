// SPDX-License-Identifier: Apache-2.0

#[test]
fn tabulated_replay_stops_at_missing_middle_separators_and_rejects_an_extra_one() {
    for middles in 0..=3 {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 8]);
        let replay_offset = payload.len();
        payload.extend_from_slice(&[
            9, 0x13, 0xe2, 0x01, 0, 3, 0x18, 0xe6, 0x0f, 0xe6, 0xf8, 4, 0xf7, 32, 0xfb, 0xe2, 0xf7,
            36,
        ]);
        for index in 0..4 {
            for _ in 0..2 {
                payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
            }
            if index == 0 {
                payload.extend_from_slice(&[0x18, 0xf1, 0xf7, 32, 0xe2]);
            } else if index <= middles {
                payload.extend_from_slice(&[0x18, 0xe2]);
            }
        }
        payload.extend_from_slice(&[0x18, 0xf2, 0xf7, 37, 0xf6, 0xe3]);
        let replays = super::work_output(|ctx| {
            crate::surface::tabulated_cylinder_curve_replays(ctx, &payload)
        });
        if middles == 2 {
            let [replay] = replays.as_slice() else {
                panic!("complete middle separators");
            };
            assert_eq!(replay.surface_id, 7);
            assert_eq!(replay.curve_id, 9);
            assert_eq!(replay.control_points, [Some([-3.0, 3.0]); 4]);
            assert_eq!(replay.body, payload[replay_offset..]);
        } else {
            assert!(replays.is_empty());
        }
    }
}
