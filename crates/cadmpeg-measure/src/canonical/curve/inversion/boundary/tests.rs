// SPDX-License-Identifier: Apache-2.0

use super::BoundaryIndex;
use crate::canonical::curve::inversion::{
    nearest_boundary_witness, BoundaryWitness, MAX_CACHED_CURVE_POINTS,
};
use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;

const FIT_TOLERANCE: f64 = 1e-8;

fn parameter(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}
fn position(value: FiniteReal) -> Option<FinitePoint3> {
    let t = value.get();
    FinitePoint3::new(Point3::new(t, t * t, -t))
}

#[test]
fn indexed_candidates_keep_the_original_witness_bits() {
    let boundaries: Vec<_> = (0..=16)
        .map(|index| parameter(f64::from(index) / 8.0 - 1.0))
        .collect();
    let index = BoundaryIndex::new(&boundaries, position).unwrap();
    for target in boundaries.iter().copied() {
        let point = position(target).unwrap().get();
        for seed in boundaries.iter().copied() {
            for tolerance in [0.0, FIT_TOLERANCE, 0.5, 2.0] {
                let original = nearest_boundary_witness(&boundaries, seed, tolerance, |at| {
                    Ok::<_, ResourceLimit>(
                        position(at).map(|position| position.get().distance(point)),
                    )
                })
                .unwrap();
                let expected = match original {
                    BoundaryWitness::Found(value) => Some(value),
                    BoundaryWitness::NoMatch => None,
                    BoundaryWitness::Invalid => unreachable!(),
                };
                assert_eq!(
                    index
                        .witness(point, seed, tolerance)
                        .map(|value| value.get().to_bits()),
                    expected.map(|value| value.get().to_bits())
                );
            }
        }
    }
}

#[test]
fn outward_radius_keeps_rounded_subtractions_at_cancellation() {
    let boundaries = [parameter(0.0), parameter(1.0)];
    let position = FinitePoint3::new(Point3::new(f64::EPSILON / 4.0, 0.0, 0.0)).unwrap();
    let point = Point3::new(-1.0, 0.0, 0.0);
    assert_eq!(position.get().distance(point), 1.0);
    let index = BoundaryIndex::new(&boundaries, |at| {
        if at.get() == 0.0 {
            Some(position)
        } else {
            FinitePoint3::new(Point3::new(2.0, 0.0, 0.0))
        }
    })
    .unwrap();
    assert_eq!(index.axis, 0);
    assert_eq!(
        index.witness(point, parameter(0.0), 1.0),
        Some(parameter(0.0))
    );
}

#[test]
fn equal_seed_distances_keep_the_first_boundary() {
    let boundaries = [parameter(0.25), parameter(0.75)];
    let index = BoundaryIndex::new(&boundaries, |at| {
        FinitePoint3::new(Point3::new(-at.get(), 0.0, 0.0))
    })
    .unwrap();
    assert_eq!(
        index.witness(Point3::new(0.0, 0.0, 0.0), parameter(0.5), 1.0),
        Some(parameter(0.25))
    );
}

#[test]
fn unavailable_and_oversized_boundary_sets_keep_the_linear_path() {
    let boundaries = [parameter(0.0), parameter(1.0)];
    assert!(BoundaryIndex::new(&boundaries, |_| None).is_none());
    let boundaries: Vec<_> = (0..=MAX_CACHED_CURVE_POINTS)
        .map(|_| parameter(0.0))
        .collect();
    assert!(BoundaryIndex::new(&boundaries, position).is_none());
}

#[test]
fn overflowing_seed_distances_keep_the_original_no_match() {
    let boundaries = [parameter(f64::MAX)];
    let point = Point3::new(0.0, 0.0, 0.0);
    let index = BoundaryIndex::new(&boundaries, |_| FinitePoint3::new(point)).unwrap();
    assert_eq!(index.witness(point, parameter(-f64::MAX), 0.0), None);
}

#[test]
fn equal_zero_boundaries_keep_the_first_parameter_bits() {
    let point = Point3::new(0.0, 0.0, 0.0);
    for zero in [0.0_f64, -0.0_f64] {
        let boundaries = [parameter(zero), parameter(-zero)];
        let index = BoundaryIndex::new(&boundaries, |_| FinitePoint3::new(point)).unwrap();
        let found = index.witness(point, parameter(0.0), 0.0).unwrap();
        assert_eq!(found.get().to_bits(), zero.to_bits());
    }
}
