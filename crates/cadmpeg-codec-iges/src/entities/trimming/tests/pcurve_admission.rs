// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::{homogeneous_pcurve_spans, split_homogeneous_pcurve};
use crate::entities::nurbs_controls::{BlendCheck, RefiningNet};

/// De Casteljau's split that keeps every level.
fn split_keeping_levels(controls: &[[f64; 4]], parameter: f64) -> (Vec<[f64; 4]>, Vec<[f64; 4]>) {
    let mut levels = vec![controls.to_vec()];
    while levels.last().unwrap().len() > 1 {
        let next = levels
            .last()
            .unwrap()
            .windows(2)
            .map(|pair| {
                std::array::from_fn(|axis| {
                    (1.0 - parameter) * pair[0][axis] + parameter * pair[1][axis]
                })
            })
            .collect();
        levels.push(next);
    }
    (
        levels.iter().map(|level| level[0]).collect(),
        levels
            .iter()
            .rev()
            .map(|level| *level.last().unwrap())
            .collect(),
    )
}

#[test]
fn pcurve_split_of_a_high_degree_polygon_matches_the_level_split() {
    let controls: Vec<[f64; 4]> = (0..1024_u32)
        .map(|index| {
            let t = f64::from(index);
            [1.0 + f64::from(index % 5) / 8.0, t.sin(), t.cos(), 0.0]
        })
        .collect();
    for parameter in [0.0, 0.37, 1.0] {
        assert_eq!(
            split_homogeneous_pcurve(&controls, parameter).unwrap(),
            split_keeping_levels(&controls, parameter)
        );
    }
}

#[test]
fn pcurve_spans_of_a_long_pcurve_insert_each_knot_near_its_span() {
    let count = 1_u32 << 17;
    let mut knots = vec![0.0; 3];
    knots.extend((1..count - 2).map(f64::from));
    knots.extend([f64::from(count - 2); 3]);
    let controls: Vec<_> = (0..count)
        .map(|index| [1.0, f64::from(index), 0.0, 0.0])
        .collect();
    let ctx = cadmpeg_test_support::service_decode_context();
    let spans = homogeneous_pcurve_spans(2, &knots, &controls, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(spans.len(), usize::try_from(count - 2).unwrap());
    assert_eq!(
        spans[1].controls,
        [
            [1.0, 1.5, 0.0, 0.0],
            [1.0, 2.0, 0.0, 0.0],
            [1.0, 2.5, 0.0, 0.0]
        ]
    );
}

#[test]
fn pcurve_knot_insertion_reuses_controls_and_preserves_homogeneous_values() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let knots = [0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
    let controls = [
        [1.0, 0.0, 0.0, 0.0],
        [2.0, 2.0, 4.0, 0.0],
        [3.0, 6.0, 0.0, 0.0],
        [4.0, 8.0, 2.0, 0.0],
        [5.0, 10.0, 0.0, 0.0],
    ];
    let mut net = RefiningNet::new(&ctx, &knots, &controls, 1).unwrap();
    assert!(net.insert(0.5, 3, BlendCheck::Denominator).is_some());
    let (knots, controls) = net.into_parts();
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
    let controls: Vec<_> = (0..CONTROL_COUNT)
        .map(|index| [1.0, f64::from(index), 0.0, 0.0])
        .collect();
    let ctx = cadmpeg_test_support::service_decode_context();
    let spans = homogeneous_pcurve_spans(2, &knots, &controls, &ctx)
        .unwrap()
        .unwrap();
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
