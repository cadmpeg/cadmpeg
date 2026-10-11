// SPDX-License-Identifier: Apache-2.0

use super::super::{circular_sweep_cylinder_from_cap_outlines, CapOutline};

const PLANES: [(u32, [f64; 3], [f64; 3]); 2] = [
    (1, [0.0; 3], [0.0, 0.0, 1.0]),
    (2, [0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
];

fn cap(index: usize, radius: f64) -> CapOutline {
    let (surface_id, origin, normal) = PLANES[index];
    CapOutline {
        surface_id,
        origin,
        normal,
        corners: [[-radius, -radius, origin[2]], [radius, radius, origin[2]]],
    }
}

#[test]
fn two_cap_selection_is_fixed_work_and_preserves_disagreement() {
    for outlines in [
        [Some(cap(0, 1.0)), None],
        [None, Some(cap(1, 1.0))],
        [Some(cap(0, 1.0)), Some(cap(1, 1.0))],
    ] {
        let cylinder =
            circular_sweep_cylinder_from_cap_outlines(PLANES, outlines).expect("unit cylinder");
        assert_eq!(cylinder.radius().get(), 1.0);
    }
    assert!(circular_sweep_cylinder_from_cap_outlines(PLANES, [None, None]).is_none());
    assert!(circular_sweep_cylinder_from_cap_outlines(
        PLANES,
        [Some(cap(0, 1.0)), Some(cap(1, 2.0))]
    )
    .is_none());
}
