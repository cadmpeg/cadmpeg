// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::Point3;

use super::{key, longer_edge};

#[test]
fn an_equilateral_triangle_splits_the_edge_with_the_lower_vertex_ids() {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.5, 3.0_f64.sqrt() / 2.0, 0.0),
    ];
    let first = (0, 1);
    let second = (1, 2);
    assert_eq!(
        longer_edge(&points, first, second),
        key(first.0, first.1).cmp(&key(second.0, second.1))
    );
}

#[test]
fn last_place_length_noise_does_not_change_the_split_edge() {
    let one = 1.0_f64;
    let above = f64::from_bits(one.to_bits() + 1);
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(one, 0.0, 0.0),
        Point3::new(above, 0.0, 0.0),
    ];
    assert_eq!(
        longer_edge(&points, (0, 1), (0, 2)),
        key(0, 1).cmp(&key(0, 2))
    );
}

#[test]
fn a_clearly_longer_edge_wins() {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
    ];
    assert_eq!(
        longer_edge(&points, (0, 1), (0, 2)),
        std::cmp::Ordering::Less
    );
}
