// SPDX-License-Identifier: Apache-2.0
use super::{BoundaryIndex, Point2};

fn brute_force(polygons: &[Vec<Point2>], tolerance: f64) -> bool {
    polygons.iter().enumerate().any(|(i, polygon)| {
        super::super::polygon_boundaries_intersect(polygon, polygon, tolerance, true)
            || polygons.iter().skip(i + 1).any(|other| {
                super::super::polygon_boundaries_intersect(polygon, other, tolerance, false)
            })
    })
}

#[test]
fn spatial_candidates_match_exact_predicates() {
    let mut state = 7919_u64;
    for _ in 0..1000 {
        let polygons: Vec<Vec<Point2>> = (0..3)
            .map(|_| {
                (0..7)
                    .map(|_| {
                        let mut coordinate = || {
                            state = state
                                .wrapping_mul(6_364_136_223_846_793_005)
                                .wrapping_add(1);
                            f64::from(u16::try_from(state >> 48).expect("upper 16 state bits"))
                                / 4096.0
                        };
                        Point2::new(coordinate(), coordinate())
                    })
                    .collect()
            })
            .collect();
        let index = BoundaryIndex::new(&polygons);
        for tolerance in [0.0, super::super::EPS_PLANAR_COORDINATE, 0.1] {
            assert_eq!(
                index.intersects(tolerance),
                brute_force(&polygons, tolerance)
            );
        }
    }
}

#[test]
fn separated_boundaries_prune_exact_segment_tests() {
    let polygons: Vec<_> = (0..512)
        .map(|i| {
            let x = f64::from(i) * 4.0;
            vec![
                Point2::new(x, 0.0),
                Point2::new(x + 1.0, 0.0),
                Point2::new(x + 1.0, 1.0),
                Point2::new(x, 1.0),
            ]
        })
        .collect();
    let index = BoundaryIndex::new(&polygons);
    let mut comparisons = 0;
    assert!(!index.any_pair(0.0, |_, _| {
        comparisons += 1;
        false
    }));
    assert_eq!(comparisons, 0);
    assert!(!index.intersects(0.0));
}

#[test]
fn spatial_candidates_keep_tolerance_boundary_contacts() {
    for scale in [f64::MIN_POSITIVE, 1.0, 1.0e100] {
        let tolerance = scale * super::super::EPS_PLANAR_COORDINATE;
        for gap in [0.0, tolerance.next_down(), tolerance, tolerance.next_up()] {
            let polygons = vec![
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(scale, 0.0),
                    Point2::new(scale, scale),
                    Point2::new(0.0, scale),
                ],
                vec![
                    Point2::new(scale + gap, 0.0),
                    Point2::new(2.0 * scale, 0.0),
                    Point2::new(2.0 * scale, scale),
                    Point2::new(scale + gap, scale),
                ],
            ];
            assert_eq!(
                BoundaryIndex::new(&polygons).intersects(tolerance),
                brute_force(&polygons, tolerance)
            );
        }
    }
}
