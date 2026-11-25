// SPDX-License-Identifier: Apache-2.0
//! The members of a fillet's transitions.

use std::collections::{BTreeMap, BTreeSet};

use super::{assert_finding, assert_no_finding_containing, edge_set, fillet, positive, with};
use crate::design::dimension::Dimension;
use crate::design::ops::dressup::{FilletSize, Transition, TransitionKind};

#[test]
fn a_swept_around_of_three_members_has_one_largest_piece() {
    let swept = |radii: [f64; 3]| {
        let sets = radii
            .iter()
            .map(|radius| {
                edge_set(Some(FilletSize::Radius(Dimension::Literal(positive(
                    *radius,
                )))))
            })
            .collect();
        let transition = Transition {
            between: BTreeSet::from([0, 1, 2]),
            at: None,
            kind: Some(TransitionKind::SweptAround {
                radius: None,
                against: None,
            }),
            native_options: BTreeMap::new(),
        };
        with(fillet(sets, vec![transition]))
    };
    let message = "no single largest piece";
    assert_no_finding_containing(&swept([1.0, 3.0, 2.0]), message);
    assert_finding(&swept([3.0, 1.0, 3.0]), message);
}
