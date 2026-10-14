// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::{homogeneous_pcurve_spans, insert_homogeneous_pcurve_knot};

#[test]
fn pcurve_knot_insertion_reuses_controls_and_preserves_homogeneous_values() {
    let mut knots = vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
    let mut controls = vec![
        [1.0, 0.0, 0.0, 0.0],
        [2.0, 2.0, 4.0, 0.0],
        [3.0, 6.0, 0.0, 0.0],
        [4.0, 8.0, 2.0, 0.0],
        [5.0, 10.0, 0.0, 0.0],
    ];
    assert!(insert_homogeneous_pcurve_knot(3, &mut knots, &mut controls, 0.5).is_some());
    assert_eq!(knots, [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(
        controls,
        [
            [1.0, 0.0, 0.0, 0.0],
            [2.0, 2.0, 4.0, 0.0],
            [2.5, 4.0, 2.0, 0.0],
            [3.5, 7.0, 1.0, 0.0],
            [4.0, 8.0, 2.0, 0.0],
            [5.0, 10.0, 0.0, 0.0],
        ]
    );
}

#[test]
fn pcurve_many_internal_knots_split_into_one_span_per_knot_interval() {
    const CONTROL_COUNT: u32 = 1000;
    let mut knots = vec![0.0; 3];
    knots.extend((1..CONTROL_COUNT - 2).map(f64::from));
    knots.extend([f64::from(CONTROL_COUNT - 2); 3]);
    let controls = (0..CONTROL_COUNT)
        .map(|index| [1.0, f64::from(index), 0.0, 0.0])
        .collect();
    let spans = homogeneous_pcurve_spans(2, &knots, controls).unwrap();
    assert_eq!(spans.len(), usize::try_from(CONTROL_COUNT - 2).unwrap());
    assert_eq!(spans.first().unwrap().domain, [0.0, 1.0]);
    assert_eq!(spans.first().unwrap().controls[0], [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(
        spans.last().unwrap().domain,
        [f64::from(CONTROL_COUNT - 3), f64::from(CONTROL_COUNT - 2)]
    );
    assert_eq!(
        spans.last().unwrap().controls[2],
        [1.0, f64::from(CONTROL_COUNT - 1), 0.0, 0.0]
    );
}
