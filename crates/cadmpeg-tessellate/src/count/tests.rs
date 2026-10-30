// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::{FRAC_PI_2, TAU};

use cadmpeg_ir::compare::FLOAT_TOLERANCE;

use super::{ceil_count, exceeds_limit, floor_count};

/// A whole number of eighth-turns must not gain a seed from last-place noise.
/// A full circle is eight seeds, a semicircle four, a quarter two.
#[test]
fn an_exact_eighth_turn_multiple_is_stable_under_last_place_noise() {
    for (eighths, expected) in [(1.0_f64, 1), (2.0, 2), (4.0, 4), (8.0, 8)] {
        for bits in [
            eighths.to_bits() - 1,
            eighths.to_bits(),
            eighths.to_bits() + 1,
        ] {
            assert_eq!(
                ceil_count(f64::from_bits(bits)),
                Some(expected),
                "{eighths} eighth-turn(s) at bits {bits:#x}"
            );
        }
    }
}

/// A sweep between two eighth-turn boundaries still rounds up.
#[test]
fn a_partial_eighth_turn_rounds_up() {
    for (eighths, expected) in [(0.5, 1), (1.5, 2), (3.5, 4), (7.5, 8)] {
        assert_eq!(
            ceil_count(eighths),
            Some(expected),
            "{eighths} eighth-turns"
        );
    }
}

/// A seed-grid ratio just above or below an integer keeps that integer, so one
/// platform's last-place noise does not add or drop a row.
#[test]
fn a_seed_grid_ratio_near_an_integer_is_stable() {
    for count in [1.0_f64, 8.0, 26.0, 64.0] {
        let expected = cadmpeg_core::convert::truncate_f64_to_usize(count).expect("fixture");
        for bits in [count.to_bits() - 1, count.to_bits(), count.to_bits() + 1] {
            assert_eq!(
                floor_count(f64::from_bits(bits)),
                Some(expected),
                "{count} intervals at bits {bits:#x}"
            );
        }
    }
}

/// A ratio well inside an interval still floors, so a 26.3-step span stays 26.
#[test]
fn a_partial_seed_grid_interval_stays_on_the_lower_side() {
    assert_eq!(floor_count(26.3), Some(26));
    assert_eq!(floor_count(0.9), Some(0));
}

#[test]
fn nonfinite_counts_are_absent() {
    assert_eq!(ceil_count(f64::NAN), None);
    assert_eq!(ceil_count(f64::INFINITY), None);
    assert_eq!(floor_count(f64::NAN), None);
    assert_eq!(floor_count(f64::NEG_INFINITY), None);
}

/// One unit in the last place above a chord or angle limit is not a split.
#[test]
fn a_limit_plus_last_place_does_not_split() {
    for limit in [0.01_f64, 0.138, 0.3, FRAC_PI_2, TAU] {
        let above = f64::from_bits(limit.to_bits() + 1);
        assert!(above > limit, "{limit}");
        assert!(!exceeds_limit(above, limit), "{limit}");
        assert!(!exceeds_limit(limit, limit), "{limit}");
    }
}

#[test]
fn a_change_beyond_the_tolerance_still_splits() {
    let limit = 0.3_f64;
    let over = limit * (1.0 + 1000.0 * FLOAT_TOLERANCE);
    assert!(exceeds_limit(over, limit));
    assert!(exceeds_limit(limit + 1.0, limit));
}
