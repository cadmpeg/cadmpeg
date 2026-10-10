// SPDX-License-Identifier: Apache-2.0

use super::super::single_plane_extrusion_extent_and_direction;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};


#[test]
fn single_plane_extent_is_fixed_work_for_both_offset_signs() {
    for sign in [-1.0, 1.0] {
        let result = single_plane_extrusion_extent_and_direction(
            [1.0, 2.0, 3.0], [0.0, 0.0, 2.0],
            ([1.0, 2.0, 3.0 + sign * 6.0], [0.0, 0.0, -3.0]),
        );
        assert_eq!(result, Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(6.0)
                            .expect("six-unit extent"),
                    },
                    draft: None,
                },
            },
            [0.0 * sign, 0.0 * sign, sign],
        )));
    }
}

#[test]
fn single_plane_extent_keeps_degenerate_and_overflow_rejections() {
    for (profile, direction, plane) in [
        ([0.0; 3], [0.0; 3], ([0.0, 0.0, 2.0], [0.0, 0.0, 1.0])),
        ([0.0; 3], [0.0, 0.0, 1.0], ([0.0, 0.0, 2.0], [0.0; 3])),
        ([0.0; 3], [0.0, 0.0, 1.0], ([0.0, 0.0, 2.0], [1.0, 0.0, 0.0])),
        ([0.0; 3], [0.0, 0.0, 1.0], ([0.0; 3], [0.0, 0.0, 1.0])),
        ([0.0, 0.0, -f64::MAX], [0.0, 0.0, 1.0],
            ([0.0, 0.0, f64::MAX], [0.0, 0.0, 1.0])),
    ] {
        assert_eq!(single_plane_extrusion_extent_and_direction( profile, direction, plane)
            , None);
    }
}

