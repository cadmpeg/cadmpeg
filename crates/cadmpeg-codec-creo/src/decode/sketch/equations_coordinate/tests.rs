// SPDX-License-Identifier: Apache-2.0

use super::super::axis::SectionAxis;
use super::{SectionCoordinateVariable, SectionEqualLengthConstraint, SectionEquationFixture};
use cadmpeg_core::decode::DecodeContext;
use std::collections::BTreeMap;

#[test]
fn coordinate_point_value_has_one_term() {
    assert_eq!(
        SectionEquationFixture::point_value(7, SectionAxis::U, 2.0)
            .terms
            .len(),
        1
    );
}

#[test]
fn coordinate_point_difference_has_two_terms() {
    assert_eq!(
        SectionEquationFixture::point_difference(7, 8, SectionAxis::V, 3.0)
            .terms
            .len(),
        2
    );
}

fn unsigned_value_fixture(
    ctx: &DecodeContext<'_>,
) -> Result<BTreeMap<SectionCoordinateVariable, f64>, cadmpeg_core::CodecError> {
    let equations = [
        SectionEquationFixture::point_value(1, SectionAxis::U, 0.0),
        SectionEquationFixture::point_value(2, SectionAxis::U, 1.0),
    ];
    let stored = BTreeMap::from([((1, SectionAxis::U), 0.0)]);
    super::solve_unsigned_dimension_coordinates(
        ctx,
        &equations,
        &stored,
        &[(1, 2, SectionAxis::U, 1.0)],
    )
}

#[test]
fn unsigned_value_fixture_preserves_the_unique_distance_solution() {
    let solved = crate::decode::with_service_context(unsigned_value_fixture)
        .expect("solve the unsigned distance");
    assert_eq!(solved, BTreeMap::from([((2, SectionAxis::U), 1.0)]));
}

#[test]
fn unsigned_distance_branches_resolve_only_a_fully_anchored_chain() {
    for fully_anchored in [false, true] {
        let mut equations = vec![SectionEquationFixture::point_value(1, SectionAxis::U, 0.0)];
        if fully_anchored {
            equations.extend((2..=13).map(|point| {
                SectionEquationFixture::point_value(point, SectionAxis::U, f64::from(point - 1))
            }));
        }
        let distances = (1..=12)
            .map(|point| (point, point + 1, SectionAxis::U, 1.0))
            .collect::<Vec<_>>();
        let stored = BTreeMap::from([((1, SectionAxis::U), 0.0)]);
        let solved = crate::decode::with_service_context(|ctx| {
            super::solve_unsigned_dimension_coordinates(ctx, &equations, &stored, &distances)
        })
        .expect("4096 sign choices share one system");
        let expected = if fully_anchored {
            (2..=13)
                .map(|point| ((point, SectionAxis::U), f64::from(point - 1)))
                .collect()
        } else {
            BTreeMap::new()
        };
        assert_eq!(solved, expected);
    }
}

#[test]
fn numerical_followup_equal_length_tangency_keeps_its_single_coordinate() {
    // Segment 1-2 spans (0.3, 0.4). Segment 3-4 is vertical and spans 0.5,
    // so the equal-length constraint has the single solution u3 = 0.0. The
    // squared lengths cancel to -2.7755575615628914e-17 instead of zero,
    // which without the cancellation rule splits the double root into
    // -5.268356063861754e-09 and 5.2683560638617535e-09 and states no
    // coordinate.
    let coordinates = BTreeMap::from([
        (1, [Some(0.0), Some(0.0)]),
        (2, [Some(0.3), Some(0.4)]),
        (3, [None, Some(0.0)]),
        (4, [Some(0.0), Some(0.5)]),
    ]);
    let constraints = [SectionEqualLengthConstraint {
        first: [1, 2],
        second: [3, 4],
        equation_id: 7,
        offset: 0,
        active: true,
    }];
    let expected: BTreeMap<SectionCoordinateVariable, Option<f64>> =
        BTreeMap::from([((3, SectionAxis::U), Some(0.0))]);
    assert_eq!(
        super::section_equal_length_coordinate_values(&constraints, &coordinates),
        expected
    );
}

/// The single U coordinate an equal-length tangency states.
///
/// Point 1 sits at the origin and point 2 at `segment`. Point 3 carries the
/// missing U coordinate and sits on the U axis; point 4 sits at
/// `(partner_u, length)`, where `length` is the length of segment 1-2. The
/// equal-length constraint 1-2 against 3-4 is then tangent, with the single
/// solution u3 = `partner_u`.
fn equal_length_tangency_u(
    segment: [f64; 2],
    partner_u: f64,
    length: f64,
) -> BTreeMap<SectionCoordinateVariable, Option<f64>> {
    let coordinates = BTreeMap::from([
        (1, [Some(0.0), Some(0.0)]),
        (2, [Some(segment[0]), Some(segment[1])]),
        (3, [None, Some(0.0)]),
        (4, [Some(partner_u), Some(length)]),
    ]);
    let constraints = [SectionEqualLengthConstraint {
        first: [1, 2],
        second: [3, 4],
        equation_id: 7,
        offset: 0,
        active: true,
    }];
    super::section_equal_length_coordinate_values(&constraints, &coordinates)
}

#[test]
fn numerical_followup_equal_length_tangency_states_its_off_axis_root() {
    // The exact constant of the quadratic is partner_u^2, and the two
    // squared segment lengths cancel on top of it. That residue is far
    // larger than a discriminant band read from the coefficient values
    // alone admits: the first and third witnesses stated no root at all and
    // the second split the double root into 0.000999992616836453 and
    // 0.0010000073831635471, which no longer agree to one coordinate.
    for (segment, partner_u, length) in [
        ([3.3, 4.3], 0.001, 5.420_332_093_147_061),
        ([0.3, 0.4], 0.001, 0.5),
        ([1.1, 2.2], 0.01, 2.459_674_775_249_769),
    ] {
        let expected: BTreeMap<SectionCoordinateVariable, Option<f64>> =
            BTreeMap::from([((3, SectionAxis::U), Some(partner_u))]);
        assert_eq!(
            equal_length_tangency_u(segment, partner_u, length),
            expected
        );
    }
}
